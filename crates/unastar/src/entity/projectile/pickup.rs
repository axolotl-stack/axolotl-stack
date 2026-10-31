//! Stuck projectiles: survival players pick up the item they carry
//! (arrows, tridents) by touching them, and they despawn after a minute.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::ActorRuntimeId;
use jolyne::valentine::{McpePacket, TakeItemActorPacket};

use super::{ProjectileItem, Stuck};
use crate::ecs::resources::TickCounter;
use crate::entity::components::{
    GameMode, Health, MainInventory, PlayerSession, Position, RuntimeEntityId, RuntimeId,
};
use crate::server::game::{player_box, send_slot};

/// Ticks a stuck projectile stays (60 s, the Java Edition arrow lifetime;
/// recorded as unmeasured).
pub const STUCK_LIFETIME: u64 = 1200;

/// System (exclusive): give touched stuck projectiles' items to players
/// and despawn the ones that stuck for [`STUCK_LIFETIME`].
#[allow(clippy::type_complexity)]
pub fn pick_up_stuck_projectiles(
    world: &mut World,
    stuck: &mut QueryState<(Entity, &Stuck, &Position, Option<&ProjectileItem>)>,
    players: &mut QueryState<
        (Entity, &Position, &GameMode, &Health),
        (
            With<MainInventory>,
            Without<crate::server::game::player_events::PendingInitialState>,
        ),
    >,
    mut due: Local<Vec<(Entity, u64, DVec3, Option<ProjectileItem>)>>,
) {
    let now = world.resource::<TickCounter>().current;
    due.extend(
        stuck
            .iter(world)
            .map(|(e, s, p, item)| (e, s.since, p.0, item.cloned())),
    );
    for (projectile, since, at, item) in due.drain(..) {
        if now.saturating_sub(since) >= STUCK_LIFETIME {
            world.despawn(projectile);
            continue;
        }
        let Some(ProjectileItem(stack)) = item else {
            continue;
        };
        let taker = players.iter(world).find(|(_, eye, mode, health)| {
            let reach = player_box(eye.0).inflate(DVec3::new(1.0, 0.5, 1.0));
            **mode != GameMode::Spectator
                && !health.is_dead()
                && (reach.min.cmple(at) & at.cmple(reach.max)).all()
        });
        let Some((player, ..)) = taker else {
            continue;
        };
        if !give(world, player, stack) {
            continue;
        }
        if let (Some(item_rid), Some(player_rid), Some(session)) = (
            world.get::<RuntimeId>(projectile).map(|r| r.0),
            world.get::<RuntimeEntityId>(player).map(|r| r.0),
            world.get::<PlayerSession>(player),
        ) {
            let _ = session.send(McpePacket::from(TakeItemActorPacket {
                item_runtime_id: ActorRuntimeId {
                    actor_runtime_id: item_rid as u64,
                },
                actor_runtime_id: ActorRuntimeId {
                    actor_runtime_id: player_rid as u64,
                },
            }));
        }
        world.despawn(projectile);
    }
}

/// Put the whole stack into the player's inventory, or nothing.
fn give(world: &mut World, player: Entity, stack: crate::item::ItemStack) -> bool {
    let Some(changed) = world.get_mut::<MainInventory>(player).and_then(|mut inv| {
        let before = inv.0.slots().to_vec();
        let mut trial = inv.0.clone();
        let (_, left) = trial.add_item(stack);
        if !left.is_empty() {
            return None;
        }
        inv.0 = trial;
        Some(
            before
                .iter()
                .zip(inv.0.slots())
                .enumerate()
                .filter(|(_, (old, new))| old != new)
                .map(|(slot, (_, new))| (slot, new.clone()))
                .collect::<Vec<_>>(),
        )
    }) else {
        return false;
    };
    for (slot, stack) in changed {
        send_slot(world, player, slot, &stack);
    }
    true
}
