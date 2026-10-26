"""Author-only marker pack for measuring BDS feature coordinate semantics.

No Mojang pack files are copied. By default this only prepares the pack; --run
uses the shared Bds helper after the caller has announced an exclusive BDS lease.
The resulting world/logs stay outside the repository.
Vanilla oak/coal probes use the documented `/place feature` command:
https://learn.microsoft.com/en-us/minecraft/creator/commands/commands/place
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import re
import shutil
import time
import uuid
from pathlib import Path

from bds_console import Console

SEED = 12345
ORIGINS = (-128, 0, 128)
DESTINATIONS = (-256, 256)


def save(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def command_result(server, command: str, success: str) -> str:
    """Collect one command between acknowledged headless console barriers."""
    result = Console(server, time.monotonic() + 40).command({"command": command}, 20)
    if any(" ERROR]" in line for line in result["response_lines"]) or not re.search(
        success, result["response"], re.IGNORECASE
    ):
        raise RuntimeError(f"{command}: {result['response']}")
    return result["response"]



def prepare(root: Path, coal_repeats: int) -> dict:
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
        ("nested_literal", "copper_block", 286, 8, 8, 1, ("1", 0, "2")),
        ("nested_random", "netherite_block", 287, 8, 8, 1, ("math.random(1, 2)", 0, "math.random(3, 4)")),
        ("nested_world", "purple_concrete", 288, 8, 8, 1, ("variable.worldx + 1", 0, "variable.worldz + 2")),
        ("nested_origin", "orange_concrete", 289, 8, 8, 1, ("variable.originx + 1", 0, "variable.originz + 2")),
        ("round_positive_low", "yellow_concrete", 290, "1.25", 0, 1, None),
        ("round_positive_high", "blue_concrete", 291, "1.75", 0, 1, None),
        ("round_negative_low", "green_concrete", 292, "-1.25", 0, 1, None),
        ("round_negative_high", "pink_concrete", 293, "-1.75", 0, 1, None),
        ("round_positive_tie", "red_concrete", 294, "1.5", 0, 1, None),
        ("round_negative_tie", "black_concrete", 295, "-1.5", 0, 1, None),
        ("heightmap", "lime_concrete", 280, 8, 8, 1, (0, "query.heightmap(variable.worldx, variable.worldz)", 0)),
        ("heightmap_clamped", "cyan_concrete", 280, 12, 8, 1, (0, "math.max(query.heightmap(variable.worldx, variable.worldz), 0)", 0)),
        ("above_top_solid", "magenta_concrete", 280, 8, 12, 1, (0, "query.above_top_solid(variable.worldx, variable.worldz)", 0)),
        ("above_top_solid_clamped", "light_blue_concrete", 280, 12, 12, 1, (0, "math.max(query.above_top_solid(variable.worldx, variable.worldz), 0)", 0)),
        ("heightmap_zero", "white_concrete", 0, 8, 16, 1, (0, "query.heightmap(variable.worldx, variable.worldz)", 0)),
        ("above_top_solid_zero", "light_gray_concrete", 0, 12, 16, 1, (0, "query.above_top_solid(variable.worldx, variable.worldz)", 0)),
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
                "may_replace": ["minecraft:air", "minecraft:stone", "minecraft:water"] if "heightmap" in name or "above_top_solid" in name else ["minecraft:air"],
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
                    "x": nested[0], "y": nested[1], "z": nested[2],
                },
            })
        save(pack / "features" / f"{name}_root.json", {
            "format_version": "1.13.0",
            "minecraft:scatter_feature": {
                "description": {"identifier": f"wg_controls:{name}_root"},
                "places_feature": places, "iterations": count,
                "coordinate_eval_order": "xzy", "x": x, "y": y, "z": z,
            },
        })
        # A rule root is required for BDS to register the authored feature
        # graph. Feature files alone were absent from /place's dynamic enum.
        save(pack / "feature_rules" / f"{name}.json", {
            "format_version": "1.13.0",
            "minecraft:feature_rules": {
                "description": {"identifier": f"wg_controls:{name}", "places_feature": f"wg_controls:{name}_root"},
                "conditions": {"placement_pass": "after_surface_pass", "minecraft:biome_filter": {"any_of": [
                    {"test": "has_biome_tag", "value": "overworld"},
                    {"test": "has_biome_tag", "value": "overworld_generation"},
                ]}},
                "distribution": {
                    "iterations": "(variable.originz == 0 && (variable.originx == -128 || variable.originx == 0 || variable.originx == 128)) ? 1 : 0",
                    "coordinate_eval_order": "xzy", "x": 0, "y": 0, "z": 0,
                },
            },
        })
        descriptions.append({"case": name, "block": f"minecraft:{block}", "y": y, "x": x, "z": z, "iterations": count, "nested": nested})
    metadata = {"seed": SEED, "origin_blocks_x": ORIGINS, "origin_blocks_z": 0, "input_y": 0, "placement_api": "place featurerule", "pack_id": pack_id, "cases": descriptions}
    metadata["requested_features"] = [
        {"feature": "minecraft:oak_tree_feature", "placement_api": "place feature", "origin": [392, 200, 8], "box": [380, 198, -4, 404, 224, 20]},
        {"feature": "minecraft:coal_ore_feature", "placement_api": "place feature", "origin": [512, 204, 8], "box": [504, 188, 0, 535, 219, 31]},
    ]
    metadata["captures"] = [
        {"name": f"wg:coal_{index:03d}", "dimension": "overworld",
         "bounds": [504, 188, 0, 535, 219, 31], "status": "planned",
         "feature": "minecraft:coal_ore_feature", "placement_api": "place feature", "origin": [512, 204, 8],
         "feature_rng_seed": "unmeasured", "iteration": index}
        for index in range(coal_repeats)
    ]
    save(root / "controls.json", metadata)
    return metadata


def run(root: Path, metadata: dict, helper_path: Path, settle_seconds: float) -> None:
    spec = importlib.util.spec_from_file_location("wg_bds_runtime", helper_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load BDS safety helper {helper_path}")
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    # This exact executable is the coordinator-sanctioned exception to the
    # loopback-bind rule: four enabled inbound Block firewall rules contain it.
    # RakNet BDS ignores server-ip; do not copy/run it from a different path.
    contained = Path("C:/Users/jvigu/orca/bds-runtime/server/bedrock_server.exe").resolve()
    if (helper.SERVER / "bedrock_server.exe").resolve() != contained:
        raise RuntimeError(f"BDS containment is only approved for {contained}")
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
            for index, x in enumerate((*ORIGINS, *DESTINATIONS)):
                server.command(f"tickingarea add {x - 16} 0 -16 {x + 31} 0 31 wg_control_{index} true")
            server.command("tickingarea add 368 0 -16 431 0 31 wg_oak true")
            server.command("tickingarea add 496 0 -16 543 0 31 wg_coal true")
            # Ticking-area preloading is asynchronous; stop then flushes the
            # world. Missing chunks are reported by the reference dumper.
            time.sleep(settle_seconds)
            for x in (*ORIGINS, *DESTINATIONS):
                server.command(f"fill {x - 16} 278 -16 {x + 31} 287 31 air")
                server.command(f"fill {x - 16} 288 -16 {x + 31} 297 31 air")
                for dx in (-16, 0, 16):
                    for z in (-16, 0, 16):
                        # A real write makes every preloaded control chunk
                        # dirty before shutdown; a no-op air fill does not.
                        server.command(f"setblock {x + dx} 260 {z} barrier")
            for x in ORIGINS:
                for dx, z in ((8, 8), (12, 8), (8, 12), (12, 12), (8, 16), (12, 16)):
                    server.command(f"fill {x + dx} -64 {z} {x + dx} 277 {z} air")
                    server.command(f"setblock {x + dx} 250 {z} stone")
                    server.command(f"setblock {x + dx} 255 {z} water")
                for case in metadata["cases"]:
                    server.command(f"place featurerule wg_controls:{case['case']} {x} 0 0")
            server.command("fill 380 200 -4 404 224 20 air")
            server.command("fill 380 198 -4 404 198 20 dirt")
            server.command("fill 380 199 -4 404 199 20 grass_block")
            server.command("place feature minecraft:oak_tree_feature 392 200 8")
            if metadata["captures"]:
                for capture in metadata["captures"]:
                    try:
                        server.command("fill 504 188 0 535 219 31 stone")
                        capture["placement_response"] = command_result(
                            server, "place feature minecraft:coal_ore_feature 512 204 8",
                            r"\bPlace command succeeded\b")
                        capture["save_response"] = command_result(
                            server, f"structure save {capture['name']} 504 188 0 535 219 31 true disk true",
                            r"\bsaved\b.*\bstructure\b|\bstructure\b.*\bsaved\b")
                        capture["status"] = "saved"
                    except (RuntimeError, TimeoutError) as error:
                        capture["status"] = "failed"
                        capture["error"] = str(error)
                        raise
            else:
                server.command("fill 504 188 0 535 219 31 stone")
                server.command("place feature minecraft:coal_ore_feature 512 204 8")
            time.sleep(8)
    finally:
        save(root / "controls.json", metadata)
        if world.exists():
            shutil.copytree(world, root / "world")
            shutil.rmtree(world)
        if installed.exists():
            shutil.rmtree(installed)
    print(f"Stopped BDS; properties restored. Diagnostic world saved at {root / 'world'}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True, type=Path, help="new directory outside the repository for packs/world/logs")
    parser.add_argument("--run", action="store_true", help="launch BDS only after announcing its exclusive lease")
    parser.add_argument("--runtime-helper", type=Path, help="tools/bds-sweep/bds_runtime.py")
    parser.add_argument("--settle-seconds", type=float, default=30)
    parser.add_argument("--coal-repeats", type=int, default=0,
                        help="save 0..64 isolated coal realizations as named structures (feature RNG seed unmeasured)")
    args = parser.parse_args()
    if not 0 <= args.coal_repeats <= 64:
        parser.error("--coal-repeats must be between 0 and 64")
    if args.run and (args.runtime_helper is None or not args.runtime_helper.is_file()):
        parser.error("--run needs --runtime-helper pointing at tools/bds-sweep/bds_runtime.py")
    root = args.out.resolve()
    metadata = prepare(root, args.coal_repeats)
    print(f"Prepared {len(metadata['cases'])} authored controls at {root}")
    if args.run:
        run(root, metadata, args.runtime_helper.resolve(), args.settle_seconds)


if __name__ == "__main__":
    main()
