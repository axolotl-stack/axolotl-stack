//! Who runs a command and the world it runs in.

use super::args::RelativeFloat;
use super::output::CommandOutput;
use super::parser::{CommandPosition, Target};
use super::schema::CommandSchema;
use super::selector::{Candidate, CarriedItem, Selector};
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{
    ArmourInventory, Experience, GameMode, MainInventory, OffhandSlot, Player, PlayerName,
    PlayerSession,
};
use crate::entity::item_drop::{EntityRandom, PLAYER_EYE_HEIGHT};
use crate::item::ItemStack;
use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::McpePacket;
use rand::Rng;
use std::sync::Arc;
use unastar_data::commands::CommandPermission;

/// Server-owned operator grant. Client command origins never confer this marker.
#[derive(Component, Debug, Clone, Copy)]
pub struct Operator;

/// Where a command came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Player(Entity),
    /// The server console (dedicated-server origin).
    Server,
}

/// The executing source: identity, permission and position.
#[derive(Debug, Clone)]
pub struct CommandSource {
    pub kind: SourceKind,
    /// BDS command permission level: players have `any`, operators
    /// `gamedirectors` (as BDS's `UpdateAbilities` reports), the console `owner`.
    pub permission: CommandPermission,
    /// Feet position (the console is at the origin).
    pub position: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Name used in messages (`Server` for the console, as BDS).
    pub name: String,
}

impl CommandSource {
    /// A player source, or `None` for an entity that is not a player.
    pub fn player(world: &World, entity: Entity) -> Option<Self> {
        world.get::<Player>(entity)?;
        let position = world.get::<Position>(entity).map_or(DVec3::ZERO, |p| p.0);
        let rotation = world.get::<Rotation>(entity).copied().unwrap_or_default();
        let name = world
            .get::<PlayerName>(entity)
            .map(|n| n.0.clone())
            .or_else(|| {
                world
                    .get::<PlayerSession>(entity)
                    .map(|s| s.display_name.clone())
            })
            .unwrap_or_default();
        Some(Self {
            kind: SourceKind::Player(entity),
            permission: player_permission(world, entity),
            position: position - DVec3::Y * PLAYER_EYE_HEIGHT,
            yaw: rotation.yaw,
            pitch: rotation.pitch,
            name,
        })
    }

    pub fn server() -> Self {
        Self {
            kind: SourceKind::Server,
            permission: CommandPermission::Owner,
            position: DVec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            name: "Server".into(),
        }
    }

    pub fn entity(&self) -> Option<Entity> {
        match self.kind {
            SourceKind::Player(entity) => Some(entity),
            SourceKind::Server => None,
        }
    }
}

/// A player's command permission: operators run `gamedirectors` commands.
pub fn player_permission(world: &World, entity: Entity) -> CommandPermission {
    if world.get::<Operator>(entity).is_some() {
        CommandPermission::GameDirectors
    } else {
        CommandPermission::Any
    }
}

/// Executor access: the whole world plus the source.
pub struct CommandContext<'w> {
    pub world: &'w mut World,
    pub source: CommandSource,
    pub schema: Arc<CommandSchema>,
}

impl<'w> CommandContext<'w> {
    pub fn new(world: &'w mut World, source: CommandSource, schema: Arc<CommandSchema>) -> Self {
        Self {
            world,
            source,
            schema,
        }
    }

    /// Resolves a target parameter to entities; an omitted optional target is
    /// the source itself (`@s`). No match is `commands.generic.noTargetMatch`,
    /// as in BDS, so executors use `ctx.entities(args.get("target"))?`.
    pub fn entities(&mut self, target: Option<&Target>) -> Result<Vec<Entity>, CommandOutput> {
        let found = match target {
            None => self.source.entity().into_iter().collect(),
            Some(Target::Name(name)) => candidates(self.world)
                .iter()
                .filter(|c| c.is_player && c.name.eq_ignore_ascii_case(name))
                .map(|c| c.entity)
                .collect(),
            Some(Target::Selector(selector)) => {
                let candidates = candidates(self.world);
                self.resolve_selector(selector, &candidates)?
            }
        };
        if found.is_empty() {
            return Err(CommandOutput::no_targets());
        }
        Ok(found)
    }

    /// Exactly one entity (`commands.generic.tooManyTargets` otherwise).
    pub fn entity(&mut self, target: Option<&Target>) -> Result<Entity, CommandOutput> {
        match self.entities(target)?.as_slice() {
            [entity] => Ok(*entity),
            _ => Err(CommandOutput::failure(
                "commands.generic.tooManyTargets",
                vec![],
            )),
        }
    }

    /// Like [`Self::entities`], but every match must be a player
    /// (`commands.generic.targetNotPlayer` otherwise).
    pub fn players(&mut self, target: Option<&Target>) -> Result<Vec<Entity>, CommandOutput> {
        let found = self.entities(target)?;
        if found.iter().any(|&e| self.world.get::<Player>(e).is_none()) {
            return Err(CommandOutput::failure(
                "commands.generic.targetNotPlayer",
                vec![],
            ));
        }
        Ok(found)
    }

    fn resolve_selector(
        &mut self,
        selector: &Selector,
        candidates: &[Candidate],
    ) -> Result<Vec<Entity>, CommandOutput> {
        let mut random = self
            .world
            .get_resource_or_insert_with(EntityRandom::default);
        let mut pick = |bound: usize| random.0.gen_range(0..bound);
        selector
            .resolve(
                candidates,
                self.source.position,
                self.source.entity(),
                &mut pick,
            )
            .map_err(|error| CommandOutput::failure(error.key(), vec![]))
    }

    /// A position parameter resolved against the source; omitted is the
    /// source's own (feet) position.
    pub fn position(&self, position: Option<&CommandPosition>) -> DVec3 {
        position.map_or(self.source.position, |p| {
            p.resolve(self.source.position, self.source.yaw, self.source.pitch)
        })
    }

    /// A block position parameter resolved against the source and floored.
    pub fn block_position(&self, position: Option<&CommandPosition>) -> IVec3 {
        self.position(position).floor().as_ivec3()
    }

    /// A rotation parameter (`yRot`/`xRot`), relative to the source's rotation.
    pub fn rotation(&self, yaw: Option<RelativeFloat>, pitch: Option<RelativeFloat>) -> (f32, f32) {
        (
            yaw.map_or(self.source.yaw, |y| y.resolve(self.source.yaw)),
            pitch.map_or(self.source.pitch, |p| p.resolve(self.source.pitch)),
        )
    }

    /// The name BDS shows for an entity: a player's name, a name tag, or the
    /// English type/item name.
    pub fn name(&self, entity: Entity) -> String {
        entity_name(self.world, entity)
    }

    /// Names joined the way BDS lists targets (`"Alice, Bob"`).
    pub fn names(&self, entities: &[Entity]) -> String {
        entities
            .iter()
            .map(|&e| self.name(e))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// BDS's message to a command's target, next to the command output
    /// (`gameMode.changed`, `commands.give.successRecipient`, …).
    pub fn notify(&self, entity: Entity, key: &str, params: Vec<String>) {
        self.send(entity, super::output::translation_text(key, &params));
    }

    /// Sends a packet to a player (no-op for other entities).
    pub fn send(&self, entity: Entity, packet: impl Into<McpePacket>) {
        if let Some(session) = self.world.get::<PlayerSession>(entity) {
            let _ = session.send(packet.into());
        }
    }

    /// Sends a packet to every connected player.
    pub fn broadcast(&mut self, packet: impl Into<McpePacket>) {
        let packet = packet.into();
        let mut sessions = self.world.query::<&PlayerSession>();
        for session in sessions.iter(self.world) {
            let _ = session.send(packet.clone());
        }
    }

    /// Feet position of an entity (players store their eye position).
    pub fn feet(&self, entity: Entity) -> Option<DVec3> {
        feet(self.world, entity)
    }
}

/// Feet position of an entity (players store their eye position).
pub fn feet(world: &World, entity: Entity) -> Option<DVec3> {
    let position = world.get::<Position>(entity)?.0;
    Some(if world.get::<Player>(entity).is_some() {
        position - DVec3::Y * PLAYER_EYE_HEIGHT
    } else {
        position
    })
}

/// Wire game type of a game mode (0 survival, 1 creative, 2 adventure, 6 spectator).
pub fn game_type_id(mode: GameMode) -> i32 {
    match mode {
        GameMode::Survival => 0,
        GameMode::Creative => 1,
        GameMode::Adventure => 2,
        GameMode::Spectator => 6,
    }
}

/// Every entity a selector can see.
pub fn candidates(world: &mut World) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut players = world.query_filtered::<Entity, With<Player>>();
    let player_entities: Vec<Entity> = players.iter(world).collect();
    for entity in player_entities {
        let Some(feet) = feet(world, entity) else {
            continue;
        };
        let rotation = world.get::<Rotation>(entity).copied().unwrap_or_default();
        out.push(Candidate {
            entity,
            is_player: true,
            type_id: "minecraft:player".into(),
            name: entity_name(world, entity),
            feet,
            yaw: rotation.yaw,
            pitch: rotation.pitch,
            game_mode: Some(game_type_id(
                world.get::<GameMode>(entity).copied().unwrap_or_default(),
            )),
            level: world.get::<Experience>(entity).map_or(0, |x| x.level),
            families: vec!["player".into()],
            items: carried_items(world, entity),
        });
    }
    let mut actors = world.query_filtered::<(Entity, &Position), Without<Player>>();
    let actors: Vec<(Entity, DVec3)> = actors.iter(world).map(|(e, p)| (e, p.0)).collect();
    for (entity, feet) in actors {
        let Some(type_id) = actor_type(world, entity) else {
            continue;
        };
        if world
            .get::<crate::entity::mob::state::Dead>(entity)
            .is_some()
        {
            continue;
        }
        let rotation = world.get::<Rotation>(entity).copied().unwrap_or_default();
        out.push(Candidate {
            entity,
            is_player: false,
            name: name_tag(world, entity).unwrap_or_default(),
            families: actor_families(world, entity),
            type_id,
            feet,
            yaw: rotation.yaw,
            pitch: rotation.pitch,
            game_mode: None,
            level: 0,
            items: Vec::new(),
        });
    }
    out
}

/// Namespaced type of a non-player actor, or `None` for entities that are not actors.
pub fn actor_type(world: &World, entity: Entity) -> Option<String> {
    if let Some(kind) = world.get::<crate::entity::mob::MobKind>(entity) {
        return Some(kind.0.identifier.to_string());
    }
    if world
        .get::<crate::entity::components::DroppedItem>(entity)
        .is_some()
    {
        return Some("minecraft:item".into());
    }
    if world.get::<crate::entity::xp_orb::XpOrb>(entity).is_some() {
        return Some("minecraft:xp_orb".into());
    }
    None
}

/// Family names of a mob (its current `type_family` membership).
fn actor_families(world: &World, entity: Entity) -> Vec<String> {
    let Some(families) = world.get::<crate::entity::definition::families::ActorFamilies>(entity)
    else {
        return Vec::new();
    };
    let Some(registry) = world.get_resource::<crate::entity::mob::MobRegistry>() else {
        return Vec::new();
    };
    let registry = registry.families();
    registry
        .names()
        .filter(|name| {
            registry
                .resolve(name)
                .is_some_and(|id| families.contains(&id).unwrap_or(false))
        })
        .map(str::to_owned)
        .collect()
}

/// A mob's name tag, if it has one.
pub fn name_tag(world: &World, entity: Entity) -> Option<String> {
    use jolyne::valentine::actor_data_i_ds as ids;
    use jolyne::valentine::types::DataItemEntryPayload;
    let metadata = world.get::<crate::entity::definition::metadata::EntityMetadata>(entity)?;
    match metadata.get(ids::NAME)? {
        DataItemEntryPayload::DataItemStringPayload(name) if !name.value.is_empty() => {
            Some(name.value.to_string())
        }
        _ => None,
    }
}

/// The name BDS shows for an entity.
pub fn entity_name(world: &World, entity: Entity) -> String {
    if world.get::<Player>(entity).is_some() {
        return world
            .get::<PlayerName>(entity)
            .map(|n| n.0.clone())
            .or_else(|| {
                world
                    .get::<PlayerSession>(entity)
                    .map(|s| s.display_name.clone())
            })
            .unwrap_or_default();
    }
    if let Some(tag) = name_tag(world, entity) {
        return tag;
    }
    if let Some(stack) = world.get::<crate::entity::components::ItemEntityStack>(entity) {
        return super::names::item_name(&stack.0);
    }
    actor_type(world, entity)
        .map(|t| super::names::entity_type_name(&t))
        .unwrap_or_default()
}

/// Items a player carries, for `hasitem`.
fn carried_items(world: &World, entity: Entity) -> Vec<CarriedItem> {
    let mut items = Vec::new();
    let mut push = |stack: &ItemStack, location: &'static str, slot: usize| {
        if !stack.is_empty() {
            items.push(CarriedItem {
                identifier: stack.item_id().to_owned(),
                aux: stack.damage as i32,
                count: stack.count as i32,
                location,
                slot: slot as i32,
            });
        }
    };
    if let Some(main) = world.get::<MainInventory>(entity) {
        for (slot, stack) in main.0.slots().iter().enumerate() {
            if slot < 9 {
                push(stack, "slot.hotbar", slot);
            } else {
                push(stack, "slot.inventory", slot - 9);
            }
        }
    }
    if let Some(armour) = world.get::<ArmourInventory>(entity) {
        for (slot, stack) in armour.slots().iter().enumerate() {
            let location = match slot {
                0 => "slot.armor.head",
                1 => "slot.armor.chest",
                2 => "slot.armor.legs",
                _ => "slot.armor.feet",
            };
            push(stack, location, 0);
        }
    }
    if let Some(offhand) = world.get::<OffhandSlot>(entity) {
        push(&offhand.0, "slot.weapon.offhand", 0);
    }
    items
}
