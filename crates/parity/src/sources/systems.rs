//! Gameplay systems: the only hand-maintained part of the denominator.
//!
//! These are cross-cutting engines with no vanilla data file to enumerate them
//! from. Add a row when vanilla has a system that no other domain covers; do
//! not duplicate things the generated domains already list (a block, item,
//! actor component, command, packet or worldgen element).

use crate::catalog::Row;

/// `(family, id, description)`.
const SYSTEMS: &[(&str, &str, &str)] = &[
    // World simulation.
    (
        "world",
        "world.block_updates",
        "neighbour updates and scheduled block ticks",
    ),
    (
        "world",
        "world.random_ticks",
        "random block ticks (growth, spread, decay, melting)",
    ),
    (
        "world",
        "world.liquids",
        "water and lava flow, sources, mixing, waterlogging",
    ),
    (
        "world",
        "world.redstone",
        "redstone power propagation and component timing",
    ),
    (
        "world",
        "world.lighting",
        "sky and block light, light updates, client light sync",
    ),
    (
        "world",
        "world.weather",
        "rain, snow, thunderstorms and lightning",
    ),
    (
        "world",
        "world.day_cycle",
        "time of day, daylight cycle, moon phases, sleeping skip",
    ),
    ("world", "world.fire", "fire spread, burnout and fire ticks"),
    (
        "world",
        "world.explosions",
        "explosion rays, block damage, entity damage and knockback",
    ),
    (
        "world",
        "world.falling_blocks",
        "gravity blocks becoming falling-block actors",
    ),
    (
        "world",
        "world.pistons",
        "piston push/pull limits and moving blocks",
    ),
    (
        "world",
        "world.chunk_ticking",
        "simulation distance, ticking areas, chunk loading",
    ),
    (
        "world",
        "world.dimensions",
        "nether and end dimensions, portals and travel between them",
    ),
    ("world", "world.world_border", "world border limits"),
    (
        "world",
        "world.difficulty",
        "difficulty and regional difficulty effects",
    ),
    (
        "world",
        "world.game_rules",
        "every game rule changes behaviour as in vanilla",
    ),
    (
        "world",
        "world.persistence",
        "saving and loading worlds, chunks, actors and players (LevelDB)",
    ),
    (
        "world",
        "world.structures_hardcoded",
        "Bedrock structures placed without shipped placement data (no data source lists them)",
    ),
    (
        "world",
        "world.structure_blocks",
        "structure blocks, structure templates and /structure",
    ),
    // Mobs and the world around them.
    (
        "mobs",
        "mobs.natural_spawning",
        "spawn caps, density, light and despawning rules shared by all spawn rules",
    ),
    (
        "mobs",
        "mobs.pathfinding",
        "navigation, path costs, doors, jumping and swimming paths",
    ),
    (
        "mobs",
        "mobs.riding",
        "passengers, seats, steering and dismounting",
    ),
    ("mobs", "mobs.leads", "leads, leash knots and lead physics"),
    (
        "mobs",
        "mobs.villages",
        "village detection, POIs, beds, bells and workstations",
    ),
    (
        "mobs",
        "mobs.raids",
        "bad omen, raid waves, raid bar and hero of the village",
    ),
    (
        "mobs",
        "mobs.patrols_and_phantoms",
        "pillager patrols, phantom spawning, wandering trader arrival",
    ),
    (
        "mobs",
        "mobs.end_fight",
        "ender dragon fight, end crystals, exit portal and gateways",
    ),
    (
        "mobs",
        "mobs.wither_summoning",
        "building and summoning the wither",
    ),
    (
        "mobs",
        "mobs.game_events",
        "vibrations and game events (sculk sensors, wardens, allays)",
    ),
    // Player survival and progression.
    (
        "player",
        "player.movement",
        "server-authoritative movement, collision and correction",
    ),
    (
        "player",
        "player.combat",
        "melee and ranged damage, critical hits, knockback, cooldowns",
    ),
    (
        "player",
        "player.damage_sources",
        "fall, drowning, suffocation, fire, lava, void, freezing and other damage causes",
    ),
    (
        "player",
        "player.hunger",
        "food, saturation, exhaustion, starvation and natural regeneration",
    ),
    (
        "player",
        "player.experience",
        "experience orbs, levels and costs",
    ),
    (
        "player",
        "player.death_respawn",
        "death, item drops, respawn points and beds/anchors",
    ),
    (
        "player",
        "player.game_modes",
        "survival, creative, adventure and spectator rules",
    ),
    (
        "player",
        "player.abilities_permissions",
        "abilities, permission levels and operator rules",
    ),
    (
        "player",
        "player.status_effects",
        "status effects, amplifiers, durations and particles",
    ),
    (
        "player",
        "player.inventory",
        "inventory transactions, item stack requests, hotbar, off-hand, armour slots",
    ),
    (
        "player",
        "player.block_breaking",
        "mining speed, tool tiers, block drops and survival break timing",
    ),
    (
        "player",
        "player.block_placing",
        "placement rules, replaceable blocks, orientation and item use on blocks",
    ),
    (
        "player",
        "player.sleeping",
        "beds, sleeping players and skipping the night",
    ),
    (
        "player",
        "player.fishing",
        "fishing rods, bobbers and fishing loot",
    ),
    (
        "player",
        "player.maps",
        "maps, map updates, locator maps and map cloning",
    ),
    (
        "player",
        "player.books",
        "book and quill editing and signing",
    ),
    (
        "player",
        "player.achievements",
        "achievements and recipe unlocks",
    ),
    // Crafting and item processing.
    (
        "crafting",
        "crafting.crafting_grid",
        "2x2 and 3x3 crafting, recipe matching and results",
    ),
    (
        "crafting",
        "crafting.recipe_book",
        "recipe book and recipe unlocking",
    ),
    (
        "crafting",
        "crafting.smelting",
        "furnace, blast furnace, smoker and campfire cooking",
    ),
    (
        "crafting",
        "crafting.enchanting",
        "enchanting table offers, bookshelves and lapis cost",
    ),
    (
        "crafting",
        "crafting.enchantments",
        "every enchantment's effect",
    ),
    (
        "crafting",
        "crafting.anvil",
        "anvil repair, combining and renaming",
    ),
    (
        "crafting",
        "crafting.grindstone",
        "grindstone repair and disenchanting",
    ),
    (
        "crafting",
        "crafting.smithing",
        "smithing upgrades and armour trims",
    ),
    (
        "crafting",
        "crafting.brewing",
        "brewing stand recipes, fuel and potion effects",
    ),
    (
        "crafting",
        "crafting.stonecutter_loom_cartography",
        "stonecutter, loom and cartography table menus",
    ),
    (
        "crafting",
        "crafting.trading",
        "villager and wandering trader offers, prices and restocking",
    ),
    (
        "crafting",
        "crafting.loot_tables",
        "loot table evaluation for blocks, chests, mobs and fishing",
    ),
    // Presentation and social.
    (
        "interface",
        "interface.chat",
        "chat, whispers and chat formatting",
    ),
    (
        "interface",
        "interface.scoreboard",
        "scoreboard objectives, displays and teams",
    ),
    (
        "interface",
        "interface.titles_hud",
        "titles, action bar, boss bars and HUD elements",
    ),
    (
        "interface",
        "interface.camera",
        "camera presets and camera instructions",
    ),
    (
        "interface",
        "interface.player_list",
        "player list, skins and emotes",
    ),
    (
        "interface",
        "interface.sounds_particles",
        "level sounds, events and particles sent for vanilla actions",
    ),
    // Server platform.
    (
        "platform",
        "platform.login_auth",
        "Xbox Live authentication, encryption and login sequence",
    ),
    (
        "platform",
        "platform.transports",
        "RakNet and NetherNet transports",
    ),
    (
        "platform",
        "platform.resource_packs",
        "resource and behaviour pack negotiation and download",
    ),
    (
        "platform",
        "platform.addons",
        "loading custom behaviour packs (actors, items, blocks, recipes)",
    ),
    (
        "platform",
        "platform.plugins",
        "plugin API and plugin loading",
    ),
    (
        "platform",
        "platform.server_config",
        "server.properties-equivalent configuration, allowlist and permissions files",
    ),
    // Performance against BDS on the same machine.
    (
        "performance",
        "performance.generation_speed",
        "chunk generation throughput and latency compared with BDS for the same world",
    ),
    (
        "performance",
        "performance.chunk_streaming",
        "time from join or movement until the view is loaded on the client, compared with BDS",
    ),
];

pub fn rows() -> Vec<Row> {
    SYSTEMS
        .iter()
        .map(|(family, id, description)| Row::new(*id, *family).with_label(*description))
        .collect()
}
