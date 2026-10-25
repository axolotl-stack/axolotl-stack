"""Prepare modest natural legacy-structure measurements; never launch by default.

The caller must announce an exclusive BDS lease before --run. No
client, packs, /place commands, or pre-existing worlds are used. Raw captures
belong outside the repository. A successful structure-save response is not a
verified DB export: inspect the stopped world with the structure exporter.

Command references (syntax, not evidence of this runtime accepting it):
https://learn.microsoft.com/en-us/minecraft/creator/commands/commands/locate
https://learn.microsoft.com/en-us/minecraft/creator/commands/commands/structure
https://learn.microsoft.com/en-us/minecraft/creator/commands/commands/tickingarea
English response patterns follow Mojang/bedrock-samples resource_pack/texts/
en_US.lang; unknown/localized responses fail closed and remain in metadata.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import math
import re
import shutil
import sys
import time
import uuid
from pathlib import Path

from bds_console import Console

# Documented locate enums. "temple" intentionally does not claim a particular
# temple subtype. Selection is explicit; there is deliberately no "all" mode.
FAMILIES = (
    "temple", "mineshaft", "stronghold", "monument", "fortress", "end_city",
    "village", "mansion", "shipwreck", "buried_treasure", "ruins",
    "pillager_outpost", "ruined_portal", "bastion_remnant",
)
HEIGHTS = {"overworld": (-64, 319), "nether": (0, 127), "the_end": (0, 255)}
LOCATE = re.compile(
    r"The nearest .+? is at block\s+(-?\d+),\s*(\(y\?\)|-?\d+),\s*(-?\d+)"
    r"\s*\(\d+(?:\.\d+)? blocks? away\)", re.IGNORECASE,
)
ERROR = re.compile(
    r"syntax error|unknown command|failed|could not|cannot|can't|unable|"
    r"no valid structure|only be used by a valid player|not found|not loaded|"
    r"outside (?:of )?the world|too (?:large|big|small)|already exists|"
    r"no ticking areas|max number of ticking areas|error", re.IGNORECASE,
)


def save(root: Path, metadata: dict) -> None:
    temporary = root / "controls.json.tmp"
    temporary.write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    temporary.replace(root / "controls.json")


def prepare(root: Path, args: argparse.Namespace) -> dict:
    repo = Path(__file__).resolve().parents[2]
    if root == repo or repo in root.parents:
        raise ValueError("--out must be outside the repository")
    families = list(dict.fromkeys(args.family or ["temple"]))
    dimensions = list(dict.fromkeys(args.dimension or ["overworld"]))
    origins = list(dict.fromkeys(tuple(p) for p in (args.origin or [[0, 64, 0]])))
    if len(families) * len(dimensions) * len(origins) > 32:
        raise ValueError("a slot plan is limited to 32 queries; split it into separate captures")
    for dimension in dimensions:
        low, high = HEIGHTS[dimension]
        if any(not low <= p[1] <= high for p in origins):
            raise ValueError(f"query Y must be inside {dimension}'s {low}..{high} range")
    run_id = uuid.uuid4().hex
    queries = []
    for family in families:
        for dimension in dimensions:
            for origin in origins:
                command = (
                    f"execute in {dimension} positioned {' '.join(map(str, origin))} "
                    f"run locate structure minecraft:{family} false"
                )
                queries.append({
                    "id": f"q{len(queries)}", "family": family,
                    "locate_identifier": f"minecraft:{family}",
                    "dimension": dimension, "query_origin": list(origin),
                    "status": "planned", "tiles": [],
                    "locate": {"command": command, "status": "planned", "coordinates": None,
                               "response": "", "response_lines": []},
                })
    metadata = {
        "schema_version": 1, "kind": "legacy_structure_controls",
        "run_id": run_id, "seed": str(args.seed), "status": "prepared",
        "capture_window": args.capture_window,
        "window": {"width": 64, "depth": 64, "tile_height": 128,
                   "offset_xz": args.window_offset,
                   "vertical_bounds_inclusive": {k: list(v) for k, v in HEIGHTS.items()},
                   "coverage": "initial bounded window, not necessarily the whole structure"},
        "timing": {"command_timeout_seconds": args.command_timeout,
                   "locate_timeout_seconds": args.locate_timeout,
                   "preload_seconds": args.preload_seconds, "slot_seconds": args.slot_seconds},
        "handoff": args.exclusive_handoff,
        "evidence_scope": "per-command responses only; template persistence/contents require stopped-DB export",
        "queries": queries, "captures": [],
        "world": {"status": "not_started", "path": "world"},
        "response_protocol": "fresh queue only; uniquely named objective add/remove acknowledgments bracket each command",
    }
    root.mkdir(parents=True, exist_ok=False)
    save(root, metadata)
    return metadata




def classify(result: dict, success: str) -> bool:
    response = result["response"]
    if not ERROR.search(response) and re.search(success, response, re.IGNORECASE):
        result["status"] = "success"
        return True
    result.update(status="failed", error="command error or unrecognized/missing success response")
    return False


def capture_query(console: Console, root: Path, metadata: dict, query: dict) -> None:
    timing = metadata["timing"]
    query["status"] = "running"
    locate = query["locate"]
    console.command(locate, timing["locate_timeout_seconds"])
    match = LOCATE.search(locate["response"])
    if ERROR.search(locate["response"]) or match is None:
        locate.update(status="failed", error="locate error or unrecognized/missing coordinate response")
        query["status"] = "failed"
        save(root, metadata)
        return
    locate["coordinates"] = [int(match[1]), None if match[2].lower() == "(y?)" else int(match[2]), int(match[3])]
    locate["status"] = "success"
    if not metadata["capture_window"]:
        query["status"] = "located"
        save(root, metadata)
        return

    # Align to chunks: exactly 4x4 chunks, rather than an accidental 5x5
    # preload. Explicit offsets let another short slot inspect an adjacent area.
    dx, dz = metadata["window"]["offset_xz"]
    x = ((locate["coordinates"][0] - 32) // 16) * 16 + dx
    z = ((locate["coordinates"][2] - 32) // 16) * 16 + dz
    dimension = query["dimension"]
    low, high = HEIGHTS[dimension]
    for index, y in enumerate(range(low, high + 1, 128)):
        name = f"wgbedrock:{metadata['run_id']}_{query['id']}_y{index}"
        bounds = [x, y, z, x + 63, y + 127, z + 63]
        capture = {
            "name": name, "dimension": dimension, "bounds": bounds,
            "status": "failed", "error": "save not attempted",
            "query_id": query["id"], "family": query["family"],
            "query_origin": query["query_origin"], "locate": locate,
            "save": {"command": f"execute in {dimension} run structure save {name} "
                     f"{' '.join(map(str, bounds[:3]))} {' '.join(map(str, bounds[3:]))} true disk true",
                     "status": "planned", "response": "", "response_lines": []},
        }
        metadata["captures"].append(capture)
        query["tiles"].append(name)
    save(root, metadata)
    area = f"wg_{query['id']}"
    preload = query["preload"] = {
        "command": f"execute in {dimension} run tickingarea add {x} {low} {z} {x + 63} {low} {z + 63} {area} true",
    }
    console.command(preload, timing["command_timeout_seconds"])
    if not classify(preload, r"Added ticking area from .+ to .+ marked for preload\."):
        query["status"] = "failed"
        save(root, metadata)
        return
    if time.monotonic() + timing["preload_seconds"] >= console.deadline:
        raise TimeoutError("slot time budget cannot accommodate the requested preload wait")
    time.sleep(timing["preload_seconds"])
    tiles = [c for c in metadata["captures"] if c["query_id"] == query["id"]]
    for capture in tiles:
        console.command(capture["save"], timing["command_timeout_seconds"])
        if classify(capture["save"], r"Saved a structure with name\s+" + re.escape(capture["name"]) + r"(?=\s|$)"):
            capture["status"] = "saved"
            del capture["error"]
        else:
            capture["error"] = capture["save"]["error"]
        save(root, metadata)
    removal = query["remove_preload"] = {"command": f"execute in {dimension} run tickingarea remove {area}"}
    console.command(removal, timing["command_timeout_seconds"])
    removed = classify(removal, r"Removed ticking area\(s\)")
    query["status"] = "captured" if removed and all(c["status"] == "saved" for c in tiles) else "failed"
    save(root, metadata)


def run(root: Path, metadata: dict, helper_path: Path) -> bool:
    server = None
    world = None
    world_owned = False
    metadata["status"] = "running"
    try:
        spec = importlib.util.spec_from_file_location("wg_legacy_bds_runtime", helper_path)
        if spec is None or spec.loader is None:
            raise RuntimeError(f"cannot load BDS safety helper {helper_path}")
        helper = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(helper)
        # Exact approved-executable guard from existing controls.py. BDS ignores
        # server-ip; only this executable has the approved firewall containment.
        contained = Path("C:/Users/jvigu/orca/bds-runtime/server/bedrock_server.exe").resolve()
        if (helper.SERVER / "bedrock_server.exe").resolve() != contained:
            raise RuntimeError(f"BDS containment is only approved for {contained}")
        if helper.bds_processes() or helper.udp_port_bound(19132) or helper.udp_port_bound(19133):
            raise RuntimeError("BDS slot is occupied; wait for BDS free before announcing a lease")
        name = f"wg-legacy-{metadata['run_id']}"
        world = helper.SERVER / "worlds" / name
        world.mkdir(parents=True, exist_ok=False)
        world_owned = True
        metadata["world"].update(status="created", temporary_path=str(world), level_name=name)
        metadata["runtime"] = {"helper": str(helper_path), "expected_version": helper.EXPECTED_VERSION,
                               "version": (helper.SERVER / "version.txt").read_text(encoding="utf-8").strip(),
                               "executable": str(contained), "endstone": False}
        properties = helper.base_properties(
            name, level_seed=metadata["seed"], level_type="DEFAULT", max_threads=1,
            tick_distance=4, view_distance=4, enable_lan_visibility="false",
        )
        server = helper.Bds(properties, root / "console.log", endstone=False)
        save(root, metadata)
        with server:
            console = Console(server, time.monotonic() + metadata["timing"]["slot_seconds"])
            for query in metadata["queries"]:
                capture_query(console, root, metadata, query)
        metadata["status"] = "complete" if all(q["status"] in ("located", "captured") for q in metadata["queries"]) else "failed"
    except BaseException as exc:
        metadata.update(status="failed", error=f"{type(exc).__name__}: {exc}")
        print(metadata["error"], file=sys.stderr)
    finally:
        for query in metadata["queries"]:
            if query["status"] in ("planned", "running"):
                query.update(status="failed", error="session ended before query completed")
            if query["locate"]["status"] in ("planned", "running"):
                query["locate"].update(status="failed", error="session ended before locate completed")
        for capture in metadata["captures"]:
            if capture["save"]["status"] in ("planned", "running"):
                capture["save"].update(status="failed", error="session ended before structure save completed")
        if server is not None and server.process is not None:
            metadata["runtime"]["returncode"] = server.process.poll()
            metadata["world"]["process_stopped"] = server.process.poll() is not None
            if server.process.poll() != 0:
                metadata.update(status="failed", shutdown_error="BDS did not exit cleanly with code zero")
        if world_owned and world is not None:
            if server is not None and server.process is not None and server.process.poll() is None:
                metadata["world"].update(status="not_copied_process_running", error="temporary world preserved in runtime")
                metadata["status"] = "failed"
            else:
                try:
                    # Only a successful copy permits removal of this invocation's
                    # unique temporary world. Copy failure leaves its source intact.
                    shutil.copytree(world, root / "world")
                    if not (root / "world" / "level.dat").is_file() or not (root / "world" / "db").is_dir():
                        raise RuntimeError("stopped world is missing level.dat or db; temporary source retained")
                    metadata["world"]["status"] = "preserved"
                    shutil.rmtree(world)
                except Exception as exc:
                    metadata["world"].update(status="preservation_failed", error=f"{type(exc).__name__}: {exc}")
                    metadata["status"] = "failed"
        save(root, metadata)
    return metadata["status"] == "complete"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", required=True, type=Path, help="NEW directory outside the repository; never overwritten")
    parser.add_argument("--run", action="store_true", help="execute this plan only after announcing the exclusive BDS lease")
    parser.add_argument("--runtime-helper", type=Path, help="explicit tools/bds-sweep/bds_runtime.py")
    parser.add_argument("--exclusive-handoff", help="exclusive BDS lease announcement reference; required by --run")
    parser.add_argument("--seed", type=int, default=12345, help="fixed signed 64-bit seed (default 12345)")
    parser.add_argument("--family", choices=FAMILIES, action="append", help="repeat for explicit families; default temple only")
    parser.add_argument("--dimension", choices=HEIGHTS, action="append", help="repeat for explicit dimensions; default overworld")
    parser.add_argument("--origin", type=int, nargs=3, action="append", metavar=("X", "Y", "Z"), help="repeat for explicit query positions; default 0 64 0; selections form a cross product")
    parser.add_argument("--capture-window", action="store_true", help="opt in to one bounded 64x64 full-height window per locate; otherwise query only")
    parser.add_argument("--window-offset", type=int, nargs=2, default=[0, 0], metavar=("DX", "DZ"), help="chunk-multiple offset from the initial window for a separate slot")
    parser.add_argument("--command-timeout", type=float, default=60)
    parser.add_argument("--locate-timeout", type=float, default=180)
    parser.add_argument("--preload-seconds", type=float, default=30, help="bounded settling wait; not proof generation completed")
    parser.add_argument("--slot-seconds", type=float, default=600, help="post-startup command budget; helper startup/shutdown take additional time")
    args = parser.parse_args()
    if args.run and (args.runtime_helper is None or not args.runtime_helper.is_file() or not args.exclusive_handoff or not args.exclusive_handoff.strip()):
        parser.error("--run requires an explicit --runtime-helper file and --exclusive-handoff lease announcement reference")
    if not -(1 << 63) <= args.seed < (1 << 63):
        parser.error("--seed must fit a signed 64-bit integer")
    if any(offset % 16 for offset in args.window_offset):
        parser.error("--window-offset values must be multiples of 16")
    if any(not math.isfinite(v) or v <= 0 for v in (args.command_timeout, args.locate_timeout, args.slot_seconds)):
        parser.error("timeouts and slot duration must be positive finite seconds")
    if not math.isfinite(args.preload_seconds) or args.preload_seconds < 0:
        parser.error("--preload-seconds must be nonnegative finite seconds")
    root = args.out.resolve()
    try:
        metadata = prepare(root, args)
    except (OSError, ValueError) as exc:
        parser.error(str(exc))
    print(f"Prepared {len(metadata['queries'])} natural-structure queries at {root}; capture_window={args.capture_window}")
    if not args.run:
        print("Prepare only: BDS was not started. --run creates and executes a NEW output directory using the same selection flags.")
        return 0
    success = run(root, metadata, args.runtime_helper.resolve())
    print(f"Capture status: {metadata['status']}; exact results: {root / 'controls.json'}")
    return 0 if success else 1


if __name__ == "__main__":
    raise SystemExit(main())
