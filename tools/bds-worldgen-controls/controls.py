"""Author-only marker pack for measuring BDS feature coordinate semantics.

No Mojang pack files are copied. By default this only prepares the pack; --run
uses TOOLING's exclusive Bds helper after the caller has acquired the BDS slot.
The resulting world/logs stay outside the repository.
Vanilla oak/coal probes use the documented `/place feature` command:
https://learn.microsoft.com/en-us/minecraft/creator/commands/commands/place
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import shutil
import time
import uuid
from pathlib import Path

SEED = 12345
ORIGINS = (-128, 0, 128)
CONDITION = "(variable.originz == 0 && (variable.originx == -128 || variable.originx == 0 || variable.originx == 128))"


def save(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def prepare(root: Path) -> dict:
    root.mkdir(parents=True, exist_ok=False)
    pack_id, module_id = str(uuid.uuid4()), str(uuid.uuid4())
    pack = root / "behavior_pack"
    save(pack / "manifest.json", {
        "format_version": 2,
        "header": {"name": "WG coordinate controls", "description": "Authored diagnostic marker rules, not vanilla assets", "uuid": pack_id, "version": [1, 0, 0], "min_engine_version": [1, 26, 50]},
        "modules": [{"type": "data", "uuid": module_id, "version": [1, 0, 0]}],
    })
    # Distinct heights/colors preserve each rule's result even when an absolute
    # coordinate causes several distant chunk origins to collide at one marker.
    cases = [
        ("numeric", "diamond_block", 280, 1, 2, 1, None),
        ("literal", "lapis_block", 281, "1", "2", 1, None),
        ("random", "gold_block", 282, "math.random(1, 2)", "math.random(3, 4)", 1, None),
        ("world", "emerald_block", 283, "variable.worldx + 4", "variable.worldz + 5", 1, None),
        ("origin", "redstone_block", 284, "variable.originx + 6", "variable.originz + 7", 1, None),
        ("uniform", "iron_block", 285, {"distribution": "uniform", "extent": [0, 16]}, 0, 128, None),
        ("nested_literal", "copper_block", 286, 8, 8, 1, ("1", "2")),
        ("nested_random", "netherite_block", 287, 8, 8, 1, ("math.random(1, 2)", "math.random(3, 4)")),
        ("nested_world", "purple_concrete", 288, 8, 8, 1, ("variable.worldx + 1", "variable.worldz + 2")),
        ("nested_origin", "orange_concrete", 289, 8, 8, 1, ("variable.originx + 1", "variable.originz + 2")),
    ]
    descriptions = []
    for name, block, y, x, z, count, nested in cases:
        marker = f"wg_controls:{name}_marker"
        save(pack / "features" / f"{name}_marker.json", {
            "format_version": "1.13.0",
            "minecraft:single_block_feature": {
                "description": {"identifier": marker},
                "places_block": f"minecraft:{block}",
                "enforce_placement_rules": False,
                "enforce_survivability_rules": False,
                "may_replace": ["minecraft:air"],
            },
        })
        places = marker
        if nested:
            places = f"wg_controls:{name}_scatter"
            save(pack / "features" / f"{name}_scatter.json", {
                "format_version": "1.13.0",
                "minecraft:scatter_feature": {
                    "description": {"identifier": places}, "places_feature": marker,
                    "iterations": 1, "coordinate_eval_order": "xzy",
                    "x": nested[0], "y": 0, "z": nested[1],
                },
            })
        save(pack / "feature_rules" / f"{name}.json", {
            "format_version": "1.13.0",
            "minecraft:feature_rules": {
                "description": {"identifier": f"wg_controls:{name}", "places_feature": places},
                "conditions": {"placement_pass": "after_surface_pass", "minecraft:biome_filter": {"any_of": [
                    {"test": "has_biome_tag", "value": "overworld"},
                    {"test": "has_biome_tag", "value": "overworld_generation"},
                ]}},
                "distribution": {"iterations": f"{CONDITION} ? {count} : 0", "scatter_chance": 100,
                                 "coordinate_eval_order": "xzy", "x": x, "y": y, "z": z},
            },
        })
        descriptions.append({"case": name, "block": f"minecraft:{block}", "y": y, "x": x, "z": z, "iterations": count, "nested": nested})
    metadata = {"seed": SEED, "origin_blocks_x": ORIGINS, "origin_blocks_z": 0, "pack_id": pack_id, "cases": descriptions}
    metadata["requested_features"] = [
        {"feature": "minecraft:oak_tree_feature", "origin": [392, 200, 8], "box": [380, 198, -4, 404, 224, 20]},
        {"feature": "minecraft:coal_ore_feature", "origin": [512, 204, 8], "box": [504, 196, 0, 520, 212, 16]},
    ]
    save(root / "controls.json", metadata)
    return metadata


def run(root: Path, metadata: dict, helper_path: Path, settle_seconds: float) -> None:
    spec = importlib.util.spec_from_file_location("wg_bds_runtime", helper_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load BDS safety helper {helper_path}")
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    suffix = metadata["pack_id"].split("-")[0]
    name = f"wg-controls-{suffix}-{SEED}"
    installed = helper.SERVER / "development_behavior_packs" / f"wg_controls_{suffix}"
    world = helper.SERVER / "worlds" / name
    # The helper checks process/port exclusivity too. Check before installing
    # any files, so a missed coordination slot cannot disturb another session.
    if helper.bds_processes() or helper.udp_port_bound(19132):
        raise RuntimeError("BDS slot is occupied; do not start until its owner hands it over")
    installed.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(root / "behavior_pack", installed)
    world.mkdir(parents=True, exist_ok=False)
    save(world / "world_behavior_packs.json", [{"pack_id": metadata["pack_id"], "version": [1, 0, 0]}])
    properties = helper.base_properties(name, level_seed=SEED, level_type="DEFAULT", max_threads=1,
                                        tick_distance=4, view_distance=8,
                                        content_log_file_enabled="true", content_log_console_output_enabled="true")
    try:
        with helper.Bds(properties, root / "console.log", endstone=False) as server:
            server.command("gamerule randomtickspeed 0")
            for index, x in enumerate(ORIGINS):
                server.command(f"tickingarea add {x - 16} 0 -16 {x + 31} 0 31 wg_control_{index} true")
            server.command("tickingarea add 368 0 -16 431 0 31 wg_oak true")
            server.command("tickingarea add 496 0 -16 543 0 31 wg_coal true")
            # Ticking-area preloading is asynchronous; stop then flushes the
            # world. Missing chunks are reported by the reference dumper.
            time.sleep(settle_seconds)
            server.command("fill 380 200 -4 404 224 20 air")
            server.command("fill 380 198 -4 404 198 20 dirt")
            server.command("fill 380 199 -4 404 199 20 grass_block")
            server.command("place feature minecraft:oak_tree_feature 392 200 8")
            server.command("fill 504 196 0 520 212 16 stone")
            server.command("place feature minecraft:coal_ore_feature 512 204 8")
            time.sleep(3)
    finally:
        if world.exists():
            shutil.copytree(world, root / "world")
            shutil.rmtree(world)
        if installed.exists():
            shutil.rmtree(installed)
    print(f"Stopped BDS; properties restored. Diagnostic world saved at {root / 'world'}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True, type=Path, help="new directory outside the repository for packs/world/logs")
    parser.add_argument("--run", action="store_true", help="launch BDS only after the coordinator grants its slot")
    parser.add_argument("--runtime-helper", type=Path, help="TOOLING tools/bds-sweep/bds_runtime.py")
    parser.add_argument("--settle-seconds", type=float, default=30)
    args = parser.parse_args()
    if args.run and (args.runtime_helper is None or not args.runtime_helper.is_file()):
        parser.error("--run needs --runtime-helper pointing at TOOLING's bds_runtime.py")
    root = args.out.resolve()
    metadata = prepare(root)
    print(f"Prepared {len(metadata['cases'])} authored controls at {root}")
    if args.run:
        run(root, metadata, args.runtime_helper.resolve(), args.settle_seconds)


if __name__ == "__main__":
    main()
