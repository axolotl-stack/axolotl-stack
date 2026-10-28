"""Entity parser/event probes against the exclusive shared BDS runtime.

Uses TOOLING's bds_runtime.Bds lifecycle helper, never a second server wrapper.
The generated pack is synthetic test input (no vanilla pack data). Each case
sets a scoreboard value through a real actor event after its filters pass.
Console queries expose those values even when no player is connected.

Generate inputs without touching BDS:
  python bds_fidelity.py --generate PATH
Run only after TOOLING releases the machine-wide BDS slot:
  python bds_fidelity.py --harness PATH/TO/tools/bds-sweep --output CACHE_DIR
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import shutil
import uuid
import time
from pathlib import Path


def eq_family(**extra):
    return {"test": "is_family", "value": "fidelity", **extra}


def probes():
    # Accepted filter spellings are exercised by a real environment sensor,
    # not merely checked for a clean startup/content log.
    cases = {
        "readable": {"all_of": [eq_family(subject="self", operator="==")]},
        "compact": {"AND": [eq_family(subject=0, operator=0)]},
        "compact_not": {"OR": [{"test": "is_family", "subject": 0, "operator": 1, "value": "absent"}]},
        "all_alias": {"all": [eq_family()]},
        "any_alias": {"any": [eq_family()]},
        "not_alias": {"NOT": [{"test": "is_family", "value": "absent"}]},
        "none_alias": {"none_of": [{"test": "is_family", "value": "absent"}]},
        "equals_alias": eq_family(operator="equals"),
        "single_equals": eq_family(operator="="),
        "not_equals": {"test": "is_family", "operator": "<>", "value": "absent"},
        "missing_subject_negative": {"none_of": [{"test": "is_family", "subject": "other", "operator": "!=", "value": "absent"}]},
        "unknown_test": {"none_of": [{"test": "fidelity_unknown_filter", "operator": "!=", "value": True}]},
        "molang_names": {"test": "bool_property", "domain": "fidelity:molang", "value": True},
        "molang_q": {"test": "bool_property", "domain": "fidelity:molang", "value": True},
        "molang_v": {"test": "bool_property", "domain": "fidelity:molang", "value": True},
        "molang_t": {"test": "bool_property", "domain": "fidelity:molang", "value": True},
        "molang_c": {"test": "bool_property", "domain": "fidelity:molang", "value": True},
        "event_nodes": eq_family(),
        "legacy_pushable": {"test": "has_component", "value": "minecraft:pushable_by_entity"},
        "null_events": eq_family(),
        "unknown_keys": eq_family(),
        "future_version": eq_family(),
        "silverfish_remove": {"test": "has_component", "value": "minecraft:is_baby"},
        "shadowed_base_remove": {"test": "has_component", "operator": "!=", "value": "minecraft:is_baby"},
        "version_engine_patch": eq_family(),
        "version_hotfix": eq_family(),
        "version_next_minor": eq_family(),
        "unknown_component": eq_family(),
        "unknown_component_old": eq_family(),
        "unknown_component_ancient": eq_family(),
        "unknown_event_action": eq_family(),
        "unknown_entity_key": eq_family(),
        "unknown_top_key": eq_family(),
        "unknown_description_key": eq_family(),
        "molang_q_default": {"test": "bool_property", "domain": "fidelity:molang", "value": True},
        "operator_not": {"test": "is_family", "operator": "not", "value": "absent"},
        "operator_lt": {"test": "int_property", "domain": "fidelity:n", "operator": "<", "value": 4},
        "operator_le": {"test": "int_property", "domain": "fidelity:n", "operator": "<=", "value": 3},
        "operator_gt": {"test": "int_property", "domain": "fidelity:n", "operator": ">", "value": 2},
        "operator_ge": {"test": "int_property", "domain": "fidelity:n", "operator": ">=", "value": 3},
        "bad_default_good_property": {"test": "int_property", "domain": "fidelity:n", "value": 3},
        "bad_default_no_property_filter": eq_family(),
    }
    for subject in range(2, 10):
        cases[f"subject_numeric_{subject}"] = {"none_of": [
            {"test": "is_family", "subject": subject, "operator": "!=", "value": "absent"}
        ]}
    for patch in range(10, 101, 10):
        cases[f"unknown_component_v21_{patch}"] = eq_family()
    documents = {}
    for name, test in cases.items():
        check = {"all_of": [
            {"test": "bool_property", "domain": "fidelity:done", "value": False}, test,
        ]}
        body = {
            "description": {
                "identifier": "fidelity:" + name,
                "runtime_identifier": "minecraft:pig",
                "is_spawnable": False,
                "is_summonable": True,
                "properties": {
                    "fidelity:done": {"type": "bool", "default": False},
                    "fidelity:molang": {"type": "bool", "default": "MaTh.ClAmP(1,0,2) == 1"},
                    "fidelity:n": {"type": "int", "range": [-10, 10], "default": 3},
                },
            },
            "components": {
                "minecraft:type_family": {"family": ["fidelity"]},
                "minecraft:health": {"value": 10, "max": 10},
                "minecraft:collision_box": {"width": 0.6, "height": 0.9},
                "minecraft:physics": {"has_gravity": False, "has_collision": True},
                "minecraft:environment_sensor": {"triggers": [{"filters": check, "event": "fidelity:pass"}]},
            },
            "events": {"fidelity:pass": {
                "set_property": {"fidelity:done": True},
                "queue_command": {"command": f"scoreboard players set {name} fidelity 1"},
            }},
        }
        aliases = {
            "molang_q": "Q.Is_Baby || !query.is_baby",
            "molang_v": "(V.unset ?? 0) == 0",
            "molang_t": "(T.unset ?? 0) == 0",
            "molang_q_default": "Q.Had_Component_Group('fidelity:absent') || !query.had_component_group('fidelity:absent')",
            "molang_c": "(C.unset ?? 0) == 0",
        }
        if name in aliases:
            body["description"]["properties"]["fidelity:molang"]["default"] = aliases[name]
        if name == "event_nodes":
            command = lambda marker: {"queue_command": {"command": [
                "scoreboard players add serial fidelity 1",
                f"scoreboard players operation {marker} fidelity = serial fidelity",
            ]}}
            body["events"]["fidelity:deferred"] = command("D")
            body["events"]["minecraft:entity_spawned"] = {
                "sequence": [
                    {"trigger": "fidelity:deferred"},
                    command("A"),
                    {"randomize": [{"weight": 1, **command("B")}]},
                    {"first_valid": [
                        {"filters": eq_family(), **command("C")},
                        command("UNEXPECTED"),
                    ]},
                ]
            }
        version = "1.26.50"
        if name == "legacy_pushable":
            version = "1.26.0"
            body["components"]["minecraft:pushable"] = {"is_pushable": True, "is_pushable_by_piston": False}
        if name == "null_events":
            # Registration is checked separately by testfor; no event marker
            # is possible when there are no events.
            body["events"] = None
            del body["components"]["minecraft:environment_sensor"]
        if name == "future_version":
            version = "99.0.0"
        if name.startswith("version_"):
            version = {
                "version_engine_patch": "1.26.51",
                "version_hotfix": "1.26.52",
                "version_next_minor": "1.27.0",
            }[name]
        if name in ["unknown_component", "unknown_component_old", "unknown_component_ancient"]:
            body["components"]["fidelity:unknown_component"] = {}
            if name == "unknown_component_old":
                version = "1.26.0"
            if name == "unknown_component_ancient":
                version = "1.21.0"
        if name.startswith("unknown_component_v21_"):
            body["components"]["fidelity:unknown_component"] = {}
            version = "1.21." + name.rsplit("_", 1)[1]
        if name.startswith("bad_default_"):
            body["description"]["properties"]["fidelity:molang"]["default"] = "query.is_baby"
            body["components"]["minecraft:environment_sensor"]["triggers"][0]["filters"] = test
            del body["events"]["fidelity:pass"]["set_property"]
        if name == "unknown_event_action":
            body["events"]["fidelity:pass"]["fidelity:unknown_action"] = {}
        if name == "unknown_entity_key":
            body["fidelity:unknown_entity_key"] = {"nested": [1, 2]}
        if name == "unknown_description_key":
            body["description"]["fidelity:unknown_description_key"] = {"nested": [1, 2]}
        if name == "unknown_keys":
            body["fidelity:unknown_entity_key"] = {"nested": [1, 2]}
            body["components"]["fidelity:unknown_component"] = {}
            body["events"]["fidelity:pass"]["fidelity:unknown_action"] = {}
        if name == "silverfish_remove":
            body["component_groups"] = {"fidelity:baby": {"minecraft:is_baby": {}}}
            body["events"]["minecraft:entity_spawned"] = {
                "sequence": [
                    {"add": {"component_groups": ["fidelity:baby"]}},
                    {"remove": {"fidelity:baby": {}}},
                ]
            }
        if name == "shadowed_base_remove":
            body["components"]["minecraft:is_baby"] = {}
            body["component_groups"] = {"fidelity:baby": {"minecraft:is_baby": {}}}
            body["events"]["minecraft:entity_spawned"] = {
                "sequence": [
                    {"add": {"component_groups": ["fidelity:baby"]}},
                    {"remove": {"component_groups": ["fidelity:baby"]}},
                ]
            }
        documents[name] = {"format_version": version, "minecraft:entity": body}
        if name == "unknown_top_key":
            documents[name]["fidelity:unknown_top_key"] = {"nested": [1, 2]}
    return documents


def generate(root: Path, definitions):
    root.mkdir(parents=True, exist_ok=False)
    header_id = str(uuid.uuid4())
    manifest = {
        "format_version": 2,
        "header": {"name": "Entity fidelity probes", "description": "Synthetic parser/event tests", "uuid": header_id, "version": [1, 0, 0], "min_engine_version": [1, 26, 51]},
        "modules": [{"type": "data", "uuid": str(uuid.uuid4()), "version": [1, 0, 0]}],
    }
    (root / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    entities = root / "entities"
    entities.mkdir()
    for name, doc in definitions.items():
        (entities / (name + ".json")).write_text(json.dumps(doc, indent=2), encoding="utf-8")
    return header_id


def check_report(report):
    baseline_path = Path(__file__).resolve().parents[1] / "tests/fixtures/bds-1.26.51-fidelity.json"
    baseline = json.loads(baseline_path.read_text(encoding="utf-8"))
    if report["bds_version"] != baseline["bds_version"] or report["probe_count"] != len(report["observations"]):
        raise SystemExit("BDS fidelity report version/count differs")
    for name, observation in report["observations"].items():
        if observation != baseline["observations"].get(name):
            raise SystemExit(f"BDS fidelity baseline differs for {name}; inspect observations.json and console.log")
    if "event_nodes" in report["observations"] and report["event_order"] != baseline["event_order"]:
        raise SystemExit("BDS fidelity deferred event order differs")
    print("BDS outcomes match the recorded 1.26.51 baseline")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--generate", type=Path)
    parser.add_argument("--harness", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--check-report", type=Path, help="verify an existing report without starting BDS")
    parser.add_argument("--only", help="comma-separated probe names for a short reserved BDS slot")
    args = parser.parse_args()
    if args.check_report:
        check_report(json.loads(args.check_report.read_text(encoding="utf-8")))
        return
    definitions = probes()
    if args.only:
        names = args.only.split(",")
        missing = set(names) - definitions.keys()
        if missing:
            parser.error(f"unknown probe names: {sorted(missing)}")
        definitions = {name: definitions[name] for name in names}
    if args.generate:
        generate(args.generate, definitions)
        print(f"Generated {len(definitions)} synthetic actor definitions at {args.generate}")
        return
    if not args.harness or not args.output:
        parser.error("provide --generate, or --harness and --output")
    spec = importlib.util.spec_from_file_location("shared_bds_runtime", args.harness / "bds_runtime.py")
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)
    if harness.bds_processes():
        raise RuntimeError("BDS is already running; coordinate the exclusive slot before probing")
    suffix = uuid.uuid4().hex
    pack = harness.SERVER / "development_behavior_packs" / ("entity_fidelity_" + suffix)
    world_name = "entity_fidelity_" + suffix
    world = harness.SERVER / "worlds" / world_name
    args.output.mkdir(parents=True, exist_ok=False)
    try:
        pack_id = generate(pack, definitions)
        world.mkdir(parents=True, exist_ok=False)
        (world / "world_behavior_packs.json").write_text(json.dumps([{"pack_id": pack_id, "version": [1, 0, 0]}]), encoding="utf-8")
        properties = harness.base_properties(world_name, content_log_file_enabled="true", content_log_console_output_enabled="true", difficulty="peaceful")
        with harness.Bds(properties, args.output / "console.log", endstone=False) as server:
            server.command("scoreboard objectives add fidelity dummy")
            server.wait_for(r"Added.*objective.*fidelity", 10)
            server.command("scoreboard players set serial fidelity 0")
            server.command("tickingarea add circle 0 80 0 2 entity_fidelity true")
            time.sleep(3)  # A preload request is not an immediate chunk-ready barrier.
            server.command("scoreboard objectives add chunks_ready dummy")
            server.wait_for(r"Added.*objective.*chunks_ready", 10)
            summon_results = {}
            for i, name in enumerate(definitions):
                server.history.clear()
                server.command(f"summon fidelity:{name} {i % 8} 80 {i // 8} 0 0 minecraft:entity_spawned probe_{name}")
                result = server.wait_for(r"Object successfully summoned|Unable to summon object|Syntax error:", 5)
                summon_results[name] = "Object successfully summoned" in result.string
            # A command after all summons synchronizes input processing. Actors
            # then receive 40 ticks before their scores are queried.
            server.command("scoreboard objectives add ready dummy")
            server.wait_for(r"Added.*objective.*ready", 10)
            time.sleep(2)

            def query(condition, objective):
                server.command(f"execute {condition} run scoreboard objectives add {objective} dummy")
                try:
                    match = server.wait_for(r"Added.*objective.*['\"]?" + objective + r"['\"]?\b", 1)
                    return match.string
                except TimeoutError:
                    return None

            observations = {}
            for i, name in enumerate(definitions):
                observations[name] = {
                    "summon_succeeded": summon_results[name],
                    "filter_pass": query(f"if score {name} fidelity matches 1", f"p{i:02}") is not None,
                }
            order = {}
            for marker in (["A", "B", "C", "D", "UNEXPECTED"] if "event_nodes" in definitions else []):
                order[marker] = next((value for value in range(1, 6)
                    if query(f"if score {marker} fidelity matches {value}", f"o{marker[0]}{value}")), None)
            report = {
                "bds_version": "1.26.51", "probe_count": len(definitions),
                "observations": observations, "event_order": order,
            }
        (args.output / "observations.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(report, indent=2))
        check_report(report)
    finally:
        # Only directories created with this invocation's random suffix.
        if pack.exists():
            shutil.rmtree(pack)
        if world.exists():
            shutil.rmtree(world)


if __name__ == "__main__":
    main()
