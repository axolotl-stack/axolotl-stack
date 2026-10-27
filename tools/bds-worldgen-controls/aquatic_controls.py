"""Measure native aquatic builtin anchoring without copying vanilla pack files.

`prepare --out <new outside-repo directory>` is offline. `run --out <directory>`
launches the sanctioned BDS only after the caller coordinates runtime/build safety.
Export stopped captures with the existing bds_structure_reference example before
interpreting placement: a command response alone does not prove block changes.
"""
from __future__ import annotations

import argparse
import csv
import importlib.util
import json
import re
import shutil
import subprocess
import sys
import time
import uuid
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SEED = 12345
X, Z = -56, -56
PANEL = (X - 24, Z - 24, X + 23, Z + 23)
BOUNDS = [PANEL[0], 0, PANEL[1], PANEL[2], 191, PANEL[3]]
FLOOR, WATER_TOP = 40, 62
FEATURES = (
    ("kelp", "minecraft:kelp_feature"),
    ("seagrass_shortest", "minecraft:shortest_seagrass_feature"),
)
ORIGINS = (
    ("floor", 41, True),
    ("mid_water", 52, True),
    ("top_water", 62, True),
    ("above_water", 63, True),
    ("air_gap", 69, True),
    # Native seagrass wrappers evaluate heightmap again below a heightmap rule;
    # measured scatter offsets add to the parent, giving 63 + 63 on this panel.
    ("nested_heightmap", 126, True),
    ("inside_floor", 40, True),
    ("buried_floor", 32, True),
    ("dry_floor", 41, False),
)


def save(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def fills(lo: int, hi: int, block: str) -> list[str]:
    """Tile inclusive vertical fills below Bedrock's 32,768-block command limit."""
    x0, z0, x1, z1 = PANEL
    rows = 32768 // ((x1 - x0 + 1) * (z1 - z0 + 1))
    return [
        f"fill {x0} {y} {z0} {x1} {min(y + rows - 1, hi)} {z1} {block}"
        for y in range(lo, hi + 1, rows)
    ]


def outside_repo(root: Path) -> Path:
    root = root.resolve()
    if root.is_relative_to(REPO):
        raise ValueError("native control worlds and captures must stay outside the repository")
    return root


def prepare(root: Path, repeats: int) -> dict:
    root = outside_repo(root)
    if not 1 <= repeats <= 32:
        raise ValueError("repeats must be between 1 and 32")
    root.mkdir(parents=True, exist_ok=False)
    cases, captures = [], []
    for label, feature in FEATURES:
        for anchor, y, wet in ORIGINS:
            for iteration in range(repeats):
                name = f"{label}_{anchor}_{iteration:03d}"
                setup = fills(FLOOR + 1, 319, "air") + fills(32, FLOOR, "stone")
                if wet:
                    setup += fills(FLOOR + 1, WATER_TOP, "water")
                # An asymmetric corner marker verifies the exported volume's frame.
                setup.append(f"setblock {PANEL[0]} {FLOOR} {PANEL[1]} gold_block")
                case = {
                    "case": name, "feature": feature, "origin": [X, y, Z],
                    "water_top": WATER_TOP if wet else None,
                    "iteration": iteration, "feature_rng_seed": "unmeasured",
                    "setup_commands": setup,
                    "placement_command": f"place feature {feature} {X} {y} {Z}",
                    "status": "planned",
                }
                cases.append(case)
                for stage in ("before", "after"):
                    captures.append({
                        "name": f"wg:aquatic_{name}_{stage}", "case": name,
                        "stage": stage, "dimension": "overworld",
                        "bounds": BOUNDS, "status": "planned",
                    })
    metadata = {
        "kind": "bedrock-aquatic-origin-controls", "schema_version": 1,
        "session_id": str(uuid.uuid4()), "seed": SEED,
        "cases": cases, "captures": captures, "commands": [],
        "reference_context": {
            "fixture": "crates/unastar/tests/fixtures/bds_1_26_51_seed_12345.json",
            "chunk": [-4, -4], "surface_biome_id": 0,
            "surface_water_columns": 256, "native_kelp_blocks": 517,
            "native_seagrass_blocks": 65,
        },
        "authored_panel": {
            "bounds_xz": list(PANEL), "solid_y": [32, FLOOR],
            "wet_y": [FLOOR + 1, WATER_TOP], "cleared_through": 319,
            "marker": [PANEL[0], FLOOR, PANEL[1]],
            "heightmap_expected_from_measured_contract": WATER_TOP + 1,
            "above_top_solid_expected_from_measured_contract": FLOOR + 1,
        },
        "interpretation": [
            "No native result until complete before/after volumes are independently exported.",
            "Verify panel, corner marker and unchanged outside/setup cells before comparing origins.",
            "A failed command or no edits is not proof of unsupported terrain or anchoring.",
            "Require at least one positive native floor-water placement before interpreting other origins.",
            "Feature RNG seed/draw order, density and natural-pass equivalence remain unmeasured.",
            "Writes touching capture boundaries require a larger capture before geometry claims.",
        ],
    }
    save(root / "controls.json", metadata)
    return metadata


def resource_guard() -> None:
    """Refuse observed competing build/server/client processes; not a substitute for a lease."""
    denied = {
        "cargo.exe", "rustc.exe", "java.exe", "javaw.exe", "bedrock_server.exe",
        "minecraft.windows.exe", "unastar.exe", "worldgen_bench.exe", "worldgen_bulk.exe",
        "baseline-debug.exe", "candidate-debug.exe", "baseline-release.exe", "candidate-release.exe",
    }
    result = subprocess.run(
        ["tasklist", "/FO", "CSV", "/NH"], capture_output=True, text=True, check=True,
    )
    busy = [row[0] for row in csv.reader(result.stdout.splitlines()) if row and row[0].casefold() in denied]
    if busy:
        raise RuntimeError(f"shared runtime/build safety prerequisite not met: {busy}")


def run(root: Path, helper_path: Path, settle_seconds: float) -> None:
    root = outside_repo(root)
    metadata = json.loads((root / "controls.json").read_text(encoding="utf-8"))
    if metadata.get("kind") != "bedrock-aquatic-origin-controls" or metadata.get("schema_version") != 1:
        raise ValueError("expected a prepared aquatic-origin controls document")
    if any(case["status"] != "planned" for case in metadata["cases"]) or (root / "world").exists():
        raise RuntimeError("refusing to overwrite an attempted capture; preserve it and prepare a new session")
    spec = importlib.util.spec_from_file_location("aquatic_bds_runtime", helper_path.resolve())
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load runtime helper {helper_path}")
    helper = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = helper
    spec.loader.exec_module(helper)
    approved = Path("C:/Users/jvigu/orca/bds-runtime/server/bedrock_server.exe").resolve()
    if (helper.SERVER / "bedrock_server.exe").resolve() != approved:
        raise RuntimeError("BDS containment is approved only for the shared server executable")
    resource_guard()
    if helper.bds_processes() or helper.udp_port_bound(19132) or helper.udp_port_bound(19133):
        raise RuntimeError("BDS runtime is occupied; never override another owner's session")
    session = str(uuid.UUID(metadata["session_id"]))
    world = helper.SERVER / "worlds" / f"wg-aquatic-{session}"
    if world.exists():
        raise RuntimeError("owned world destination already exists; inspect instead of deleting")
    world.mkdir()
    properties = helper.base_properties(
        world.name, level_seed=metadata["seed"], level_type="DEFAULT", max_threads=1,
        tick_distance=4, view_distance=8,
        content_log_file_enabled="true", content_log_console_output_enabled="true",
    )
    # Importing Console here keeps prepare entirely offline and stdlib-only.
    from bds_console import Console
    try:
        with helper.Bds(properties, root / "console.log", endstone=False) as server:
            console = Console(server, time.monotonic() + 600)

            def command(value: str) -> dict:
                result = {"command": value}
                metadata["commands"].append(result)
                console.command(result, 20)
                return result

            def require(value: str, success: str) -> dict:
                result = command(value)
                if not re.search(success, result["response"], re.IGNORECASE):
                    raise RuntimeError(result)
                return result

            for value in ("gamerule randomtickspeed 0", "gamerule dodaylightcycle false", "time set noon", "weather clear"):
                result = command(value)
                if " ERROR]" in result["response"]:
                    raise RuntimeError(result)
            require(f"tickingarea add {PANEL[0]} 0 {PANEL[1]} {PANEL[2]} 0 {PANEL[3]} wg_aquatic true", r"added.*ticking area")
            time.sleep(settle_seconds)
            for case in metadata["cases"]:
                case["status"] = "running"
                save(root / "controls.json", metadata)
                for value in case["setup_commands"]:
                    require(value, r"(?:blocks? (?:filled|changed)|successfully (?:filled|placed)|block placed)")
                for stage in ("before", "after"):
                    if stage == "after":
                        placed = command(case["placement_command"])
                        case["placement_response"] = placed
                        if re.search(r"\bplace command succeeded\b", placed["response"], re.IGNORECASE):
                            case["command_outcome"] = "reported_success"
                        elif re.search(r"could not place (?:the )?feature|place command failed|feature could not be placed", placed["response"], re.IGNORECASE):
                            case["command_outcome"] = "reported_no_placement"
                        else:
                            raise RuntimeError(placed)
                    capture = next(item for item in metadata["captures"] if item["case"] == case["case"] and item["stage"] == stage)
                    capture["save_response"] = require(
                        "structure save " + capture["name"] + " " + " ".join(map(str, capture["bounds"])) + " true disk true",
                        r"\bsaved\b.*\bstructure\b|\bstructure\b.*\bsaved\b",
                    )
                    capture["status"] = "saved"
                case["status"] = "saved"
                save(root / "controls.json", metadata)
            errors = [line for line in server.history if "[FeatureRegistry]" in line and " ERROR]" in line]
            if errors:
                raise RuntimeError(errors)
    finally:
        save(root / "controls.json", metadata)
        if world.exists():
            # Failed copies preserve the live-directory path for recovery.
            shutil.copytree(world, root / "world")
            shutil.rmtree(world)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("prepare", "run"))
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--runtime-helper", type=Path, default=REPO / "tools/bds-sweep/bds_runtime.py")
    parser.add_argument("--settle-seconds", type=float, default=30)
    args = parser.parse_args()
    if args.settle_seconds < 0:
        parser.error("settle-seconds cannot be negative")
    if args.mode == "prepare":
        metadata = prepare(args.out, args.repeats)
        print(f"Prepared {len(metadata['cases'])} aquatic cases/{len(metadata['captures'])} captures offline at {args.out}")
    else:
        run(args.out, args.runtime_helper, args.settle_seconds)
        print(f"Stopped BDS and preserved aquatic controls at {args.out}")


if __name__ == "__main__":
    main()
