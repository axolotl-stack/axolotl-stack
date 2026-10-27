use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use jolyne::valentine::bedrock::codec::{BedrockCodec, VarUInt};
use jolyne::valentine::types::CerealizerNetworkItemInstanceDescriptorSerializedData as Item;
use jolyne::valentine::{CraftingDataPacket, McpePacketData};
use sha2::{Digest, Sha256};

#[test]
fn real_join_crafting_advertisement_matches_bds_1_26_51() {
    let Ok(capture) = std::env::var("UNASTAR_CRAFTING_DATA_CAPTURE") else {
        eprintln!(
            "skipping external capture parity: set UNASTAR_CRAFTING_DATA_CAPTURE to the pinned CraftingData .bin"
        );
        return;
    };
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || run(&capture))
        .unwrap()
        .join()
        .expect("parity worker")
        .expect("real join CraftingData matches pinned BDS capture");
}

fn run(capture: &str) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = runtime.enter();
    let bytes = std::fs::read(capture)?;
    ensure!(
        format!("{:x}", Sha256::digest(&bytes))
            == "68bbcbf58c0adc94edd90fbb4fbabf1420d5a388e14b8108c1c4aa907e594923",
        "pinned capture SHA-256"
    );
    let mut input = bytes.as_slice();
    ensure!(
        VarUInt::decode(&mut input, ())?.0 == 52,
        "CraftingData packet header"
    );
    let mut bds = CraftingDataPacket::decode(&mut input, ())?;
    ensure!(input.is_empty(), "capture consumed exactly");

    let mut server = unastar::GameServer::with_config(unastar::ServerConfig {
        world: unastar::world::WorldConfig::void_spawn_platform(1),
        ..Default::default()
    });
    let (tx, mut rx) = tokio::sync::mpsc::channel(128);
    server.spawn_player(unastar::PlayerSpawnData {
        session_id: 1,
        display_name: "recipe-parity-smoke".into(),
        xuid: None,
        uuid: Some(uuid::Uuid::nil().to_string()),
        runtime_id: 1,
        position: glam::DVec3::new(0.5, 17.0, 0.5),
        saved_data: None,
        outbound_tx: tx,
        chunk_radius: 0,
    });
    let mut advertised = None;
    let mut order = Vec::new();
    while let Ok(packet) = rx.try_recv() {
        order.push(packet.header.id);
        if let McpePacketData::CraftingDataPacket(packet) = packet.data {
            ensure!(
                advertised.replace(*packet).is_none(),
                "exactly one CraftingData advertisement"
            );
        }
    }
    let advertised = advertised.context("real player join advertised CraftingData")?;
    println!("actual join outbound order: {order:?}");
    let mut encoded = Vec::new();
    advertised.encode(&mut encoded)?;
    println!(
        "actual join CraftingData payload encoded {} bytes",
        encoded.len()
    );
    let mut payload = encoded.as_slice();
    let mut ours = CraftingDataPacket::decode(&mut payload, ())?;
    ensure!(
        payload.is_empty(),
        "advertisement wire payload consumed exactly"
    );
    ensure!(
        uuid_partitions(&ours) == uuid_partitions(&bds),
        "normal recipe UUID equivalence classes match"
    );
    let ids = recipe_ids(&ours);
    let mut unique = HashSet::new();
    ensure!(
        ids.iter().all(|id| *id != 0 && unique.insert(*id)),
        "advertised recipe ids are nonzero and unique"
    );
    println!(
        "UUID equivalence classes match; {} unique advertised recipe ids",
        ids.len()
    );

    let native_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../valentine_gen/protocolgen/data/source/inputs/1.26.51/native/block_state_data.json",
    );
    let states: HashMap<String, serde_json::Value> =
        serde_json::from_slice(&std::fs::read(native_path)?)?;
    let hashes: HashMap<u32, i32> = states
        .into_iter()
        .map(|(state, data)| {
            Ok((
                data["hashedNetworkId"]
                    .as_u64()
                    .context("native state hash")? as u32,
                state.parse()?,
            ))
        })
        .collect::<Result<_>>()?;
    canonicalize(&mut bds, Some(&hashes));
    canonicalize(&mut ours, None);
    let mut differences = 0;
    differences += compare("shaped", &ours.shaped_recipes, &bds.shaped_recipes);
    differences += compare("shapeless", &ours.shapeless_recipes, &bds.shapeless_recipes);
    differences += compare(
        "user-data",
        &ours.user_data_shapeless_recipes,
        &bds.user_data_shapeless_recipes,
    );
    differences += compare("multi", &ours.multi_recipes, &bds.multi_recipes);
    differences += compare(
        "smithing transform",
        &ours.smithing_transform_recipes,
        &bds.smithing_transform_recipes,
    );
    differences += compare(
        "smithing trim",
        &ours.smithing_trim_recipes,
        &bds.smithing_trim_recipes,
    );
    differences += compare("potion mixes", &ours.potion_mixes, &bds.potion_mixes);
    differences += compare(
        "container mixes",
        &ours.container_mixes,
        &bds.container_mixes,
    );
    differences += compare(
        "shaped chemistry",
        &ours.shaped_chemistry_recipes,
        &bds.shaped_chemistry_recipes,
    );
    differences += compare(
        "shapeless chemistry",
        &ours.shapeless_chemistry_recipes,
        &bds.shapeless_chemistry_recipes,
    );
    differences += compare(
        "material reducers",
        &ours.material_reducers,
        &bds.material_reducers,
    );
    ensure!(
        ours.clear_recipes == bds.clear_recipes,
        "clear_recipes matches capture"
    );
    ensure!(
        differences == 0,
        "{differences} semantic recipe differences"
    );
    println!(
        "PASS: every CraftingData field matches after unordered rows, transient recipe IDs/UUIDs, and negotiated block-state hash conversion"
    );
    Ok(())
}

fn canonicalize(packet: &mut CraftingDataPacket, hashes: Option<&HashMap<u32, i32>>) {
    let result = |item: &mut Item| {
        if item.block_runtime_id != 0
            && let Some(hashes) = hashes
        {
            item.block_runtime_id = *hashes
                .get(&(item.block_runtime_id as u32))
                .expect("captured block state in native palette");
        }
    };
    for recipe in packet
        .shaped_recipes
        .iter_mut()
        .chain(&mut packet.shaped_chemistry_recipes)
    {
        recipe.net_id.raw_id = 0;
        recipe.uuid = uuid::Uuid::nil();
        for item in &mut recipe.results {
            result(item);
        }
    }
    for recipe in packet
        .shapeless_recipes
        .iter_mut()
        .chain(&mut packet.user_data_shapeless_recipes)
        .chain(&mut packet.shapeless_chemistry_recipes)
    {
        recipe.net_id.raw_id = 0;
        recipe.uuid = uuid::Uuid::nil();
        for item in &mut recipe.results {
            result(item);
        }
    }
    for recipe in &mut packet.smithing_transform_recipes {
        recipe.net_id.raw_id = 0;
        result(&mut recipe.result);
    }
    for recipe in &mut packet.smithing_trim_recipes {
        recipe.net_id.raw_id = 0;
    }
    for recipe in &mut packet.multi_recipes {
        recipe.net_id.raw_id = 0;
    }
    packet
        .shaped_recipes
        .sort_by(|a, b| a.recipe_id.cmp(&b.recipe_id).then(a.tag.cmp(&b.tag)));
    packet
        .shapeless_recipes
        .sort_by(|a, b| a.recipe_id.cmp(&b.recipe_id).then(a.tag.cmp(&b.tag)));
    packet
        .user_data_shapeless_recipes
        .sort_by(|a, b| a.recipe_id.cmp(&b.recipe_id).then(a.tag.cmp(&b.tag)));
    packet
        .smithing_transform_recipes
        .sort_by(|a, b| a.recipe_id.cmp(&b.recipe_id).then(a.tag.cmp(&b.tag)));
    packet
        .smithing_trim_recipes
        .sort_by(|a, b| a.recipe_id.cmp(&b.recipe_id).then(a.tag.cmp(&b.tag)));
    packet.multi_recipes.sort_by_key(|r| r.multi_recipe_uuid);
    packet.potion_mixes.sort_by_key(|r| {
        (
            r.from_potion_id,
            r.from_item_aux,
            r.reagent_item_id,
            r.reagent_item_aux,
            r.to_potion_id,
            r.to_item_aux,
        )
    });
    packet
        .container_mixes
        .sort_by_key(|r| (r.from_item_id, r.reagent_item_id, r.to_item_id));
}

fn compare<T: PartialEq + Debug>(name: &str, ours: &[T], bds: &[T]) -> usize {
    println!("{name}: ours={} BDS={}", ours.len(), bds.len());
    let mut errors = usize::from(ours.len() != bds.len());
    for (a, b) in ours.iter().zip(bds) {
        if a != b {
            errors += 1;
            if errors <= 4 {
                println!("{name} mismatch expected {b:?}\nactual {a:?}");
            }
        }
    }
    errors
}

fn uuid_partitions(packet: &CraftingDataPacket) -> Vec<Vec<String>> {
    let mut groups: HashMap<uuid::Uuid, Vec<String>> = HashMap::new();
    for (kind, rows) in [
        ("shaped", &packet.shaped_recipes),
        ("shaped-chemistry", &packet.shaped_chemistry_recipes),
    ] {
        for recipe in rows {
            groups
                .entry(recipe.uuid)
                .or_default()
                .push(format!("{kind}/{}/{}", recipe.tag, recipe.recipe_id));
        }
    }
    for (kind, rows) in [
        ("shapeless", &packet.shapeless_recipes),
        ("user-data", &packet.user_data_shapeless_recipes),
        ("shapeless-chemistry", &packet.shapeless_chemistry_recipes),
    ] {
        for recipe in rows {
            groups
                .entry(recipe.uuid)
                .or_default()
                .push(format!("{kind}/{}/{}", recipe.tag, recipe.recipe_id));
        }
    }
    let mut partitions: Vec<_> = groups
        .into_values()
        .map(|mut rows| {
            rows.sort();
            rows
        })
        .collect();
    partitions.sort();
    partitions
}

fn recipe_ids(packet: &CraftingDataPacket) -> Vec<u32> {
    let mut ids = Vec::new();
    for rows in [&packet.shaped_recipes, &packet.shaped_chemistry_recipes] {
        ids.extend(rows.iter().map(|row| row.net_id.raw_id));
    }
    for rows in [
        &packet.shapeless_recipes,
        &packet.user_data_shapeless_recipes,
        &packet.shapeless_chemistry_recipes,
    ] {
        ids.extend(rows.iter().map(|row| row.net_id.raw_id));
    }
    ids.extend(packet.multi_recipes.iter().map(|row| row.net_id.raw_id));
    ids.extend(
        packet
            .smithing_transform_recipes
            .iter()
            .map(|row| row.net_id.raw_id),
    );
    ids.extend(
        packet
            .smithing_trim_recipes
            .iter()
            .map(|row| row.net_id.raw_id),
    );
    ids
}
