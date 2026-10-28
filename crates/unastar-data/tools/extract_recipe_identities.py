"""Extract recipe identity measurements from a pinned BDS CraftingData capture.

Usage: python tools/extract_recipe_identities.py path/to/00054-CraftingDataPacket.json
Run from crates/unastar-data. The JSON is the bds-extractor debug projection;
its adjacent .bin must match the retained 1.26.51 manifest hash.
"""

import hashlib
import base64
import json
from pathlib import Path
import sys

PIN = "68bbcbf58c0adc94edd90fbb4fbabf1420d5a388e14b8108c1c4aa907e594923"
capture_path = Path(sys.argv[1])
assert hashlib.sha256(capture_path.with_suffix(".bin").read_bytes()).hexdigest() == PIN
capture = json.loads(capture_path.read_text(encoding="utf-8"))
items = json.loads(Path("src/items.json").read_text(encoding="utf-8"))
by_id = {item["network_id"]: item["identifier"] for item in items}
known_names = set(by_id.values())
rows = {}
for family in ("shaped_recipes", "shapeless_recipes"):
    for recipe in capture[family]:
        rows.setdefault(recipe["recipe_id"], []).append(recipe)


def item_identity(value, is_result):
    if isinstance(value, str):
        name, aux = value, None
    else:
        if "item" not in value:
            return None
        name, aux = value["item"], value.get("data")
    base, separator, last = name.rpartition(":")
    if separator and ":" in base and last.lstrip("-").isdigit():
        name, aux = base, int(last) if aux is None else aux
    if ":" not in name:
        name = "minecraft:" + name
    return name, 0 if is_result and aux is None else aux


def record(table, source, wire, is_result):
    identity = item_identity(source, is_result)
    if identity is None:
        return
    name, data = identity
    if is_result:
        canonical, aux = by_id[wire["id"]], wire["auxvalue"]
    else:
        descriptor = wire["descriptor"]
        assert len(descriptor) == 1 and descriptor[0]["key"] == "name", (source, wire)
        canonical = descriptor[0]["value"]
        aux = None if wire["aux_value"] == 32767 else wire["aux_value"]
    if (name, data) == (canonical, aux):
        return
    legacy = name + (f":{data}" if data is not None else "")
    value = {"name": canonical, "data": aux}
    assert legacy not in table or table[legacy] == value, (legacy, table.get(legacy), value)
    table[legacy] = value


documents = {}
for path in sorted(Path("data/vanilla_bp/behavior_pack/recipes").rglob("*.json")):
    document = json.loads(path.read_text(encoding="utf-8"))
    version = tuple(int(part) for part in (document["format_version"].split(".") + ["0", "0"])[:3])
    for kind, body in document.items():
        if kind == "format_version":
            continue
        identifier = body["description"]["identifier"]
        if identifier not in documents or documents[identifier][0] <= version:
            documents[identifier] = version, kind, body
inputs, outputs = {}, {}
for identifier, (_, kind, body) in documents.items():
    if kind not in ("minecraft:recipe_shaped", "minecraft:recipe_shapeless", "minecraft:recipe_furnace"):
        continue
    wire = rows[identifier][0]
    if kind == "minecraft:recipe_shaped":
        pattern = body["pattern"]
        width = max(map(len, pattern))
        sources = [body.get("key", {}).get(symbol) for line in pattern for symbol in line.ljust(width)]
    elif kind == "minecraft:recipe_shapeless":
        # BDS expands a counted ingredient into independent unit descriptors.
        sources = [source for source in body["ingredients"]
                   for _ in range(source.get("count", 1) if isinstance(source, dict) else 1)]
    else:
        sources = [body["input"]]
    assert len(sources) == len(wire["ingredients"]), identifier
    for source, ingredient in zip(sources, wire["ingredients"]):
        if source is not None:
            record(inputs, source, ingredient, False)
    results = body.get("result", body.get("output"))
    results = results if isinstance(results, list) else [results]
    assert len(results) == len(wire["results"]), identifier
    for source, result in zip(results, wire["results"]):
        record(outputs, source, result, True)

result = {
    "source": "BDS 1.26.51 member/join/00054-CraftingDataPacket.bin",
    "source_sha256": PIN,
    "inputs": dict(sorted(inputs.items())),
    "outputs": dict(sorted(outputs.items())),
}
Path("data/measured/recipe_legacy_identities.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8", newline="\n")
print(f"Extracted {len(inputs)} input and {len(outputs)} output legacy identities")

# Potion type identifiers have no native runtime-id table. Recover their aux
# values by matching the complete labelled brewing graph, not a hand-written
# numeric enum or a guessed seed. Case variants in the shipped pack are aliases.
legacy_names = {}
for item in items:
    if item["description_id"].startswith("item."):
        legacy_names[f"minecraft:{item['description_id'][5:]}"] = item["identifier"]
edges = []
for path in sorted(Path("data/vanilla_bp/behavior_pack/recipes").rglob("*.json")):
    document = json.loads(path.read_text(encoding="utf-8"))
    mix = document.get("minecraft:recipe_brewing_mix")
    if mix is not None:
        reagent = mix["reagent"]
        reagent = reagent if reagent in known_names else legacy_names[reagent]
        edges.append((mix["input"].lower(), reagent, mix["output"].lower()))
wire_edges = {}
for row in capture["potion_mixes"]:
    # All three potion containers carry the same labelled graph.
    if by_id[row["from_potion_id"]] != "minecraft:potion":
        continue
    reagent = by_id[row["reagent_item_id"]]
    wire_edges.setdefault(reagent, set()).add((row["from_item_aux"], row["to_item_aux"]))
codes = {code for pairs in wire_edges.values() for pair in pairs for code in pair}
domains = {name: set(codes) for source, _, target in edges for name in (source, target)}
changed = True
while changed:
    changed = False
    for source, reagent, target in edges:
        pairs = {(a, b) for a, b in wire_edges[reagent] if a in domains[source] and b in domains[target]}
        for name, allowed in ((source, {a for a, _ in pairs}), (target, {b for _, b in pairs})):
            narrowed = domains[name] & allowed
            if narrowed != domains[name]:
                domains[name] = narrowed
                changed = True
    # Distinct normalized potion types have distinct aux values.
    singletons = {next(iter(domain)) for domain in domains.values() if len(domain) == 1}
    for name, domain in domains.items():
        if len(domain) > 1 and domain - singletons != domain:
            domains[name] = domain - singletons
            changed = True
assert all(len(domain) == 1 for domain in domains.values()), domains
potions = {name: next(iter(domain)) for name, domain in sorted(domains.items())}
assert {(potions[a], reagent, potions[b]) for a, reagent, b in edges} == {
    (a, reagent, b) for reagent, pairs in wire_edges.items() for a, b in pairs
}
Path("data/measured/potion_types.json").write_text(
    json.dumps({"source_sha256": PIN, "types": potions}, indent=2) + "\n",
    encoding="utf-8", newline="\n",
)
print(f"Uniquely solved all {len(potions)} potion types from the captured brewing graph")

# Built-in user-data recipes do not exist in pack JSON. Retain only typed
# gameplay facts, replacing transient recipe UUIDs and network ids with a stable
# group name. Block runtime hashes become canonical state ordinals so the
# negotiated sequential-state protocol is not contaminated by captured hashes.
native_states = json.loads(Path("../valentine_gen/protocolgen/data/source/inputs/1.26.51/native/block_state_data.json").read_text(encoding="utf-8"))
state_by_hash = {row["hashedNetworkId"]: int(state) for state, row in native_states.items()}


def measured_ingredient(ingredient):
    descriptor = ingredient["descriptor"]
    assert len(descriptor) <= 1, ingredient
    field = descriptor[0] if descriptor else {"key": "", "value": ""}
    assert field["key"] in ("", "name", "item_tag")
    return [field["key"], field["value"], ingredient["aux_value"], ingredient["stack_size"]]


def measured_result(output):
    identifier = by_id[output["id"]]
    block_hash = output["block_runtime_id"] & 0xFFFFFFFF
    buffer = bytes.fromhex(output["user_data_buffer"])
    default = bytes(18 if identifier == "minecraft:shield" else 10)
    return [identifier, output["auxvalue"], output["stacksize"],
            None if block_hash == 0 else state_by_hash[block_hash],
            None if buffer == default else base64.b64encode(buffer).decode("ascii")]


groups = {}
for family in ("shaped_recipes", "shapeless_recipes", "user_data_shapeless_recipes"):
    for recipe in capture[family]:
        groups.setdefault(recipe["uuid"], []).append(recipe["recipe_id"])
group_names = {uuid: min(identifiers) for uuid, identifiers in groups.items()}


def measured_recipe(recipe):
    assert recipe["tag"] == "crafting_table"
    assert recipe["unlocking_requirement"]["unlocking_context"] is None
    return {
        "identifier": recipe["recipe_id"],
        "group": group_names[recipe["uuid"]],
        "ingredients": [measured_ingredient(i) for i in recipe["ingredients"]],
        "results": [measured_result(output) for output in recipe["results"]],
        "unlock": [measured_ingredient(i) for i in recipe["unlocking_requirement"]["unlocking_ingredients"]],
        "priority": recipe["priority"],
    }


builtins = [measured_recipe(recipe) for recipe in capture["user_data_shapeless_recipes"]]
native_shapeless = [measured_recipe(recipe) for recipe in capture["shapeless_recipes"]
                    if recipe["recipe_id"] not in documents]
native_shaped = [{
    "recipe": measured_recipe(recipe),
    "width": recipe["width"], "height": recipe["height"],
    "assume_symmetry": recipe["assume_symmetry"],
} for recipe in capture["shaped_recipes"] if recipe["recipe_id"] not in documents]
result_user_data = {}
for family in ("shaped_recipes", "shapeless_recipes"):
    for recipe in capture[family]:
        if recipe["recipe_id"] not in documents:
            continue
        results = [measured_result(output) for output in recipe["results"]]
        if any(result[4] is not None for result in results):
            previous = result_user_data.setdefault(recipe["recipe_id"], results)
            assert previous == results, recipe["recipe_id"]
result_states = {}
for family in ("shaped_recipes", "shapeless_recipes", "user_data_shapeless_recipes", "smithing_transform_recipes"):
    for recipe in capture[family]:
        for output in recipe.get("results", [recipe.get("result")]):
            block_hash = output["block_runtime_id"] & 0xFFFFFFFF
            identity = f"{by_id[output['id']]}:{output['auxvalue']}"
            state = None if block_hash == 0 else state_by_hash[block_hash]
            assert identity not in result_states or result_states[identity] == state, identity
            result_states[identity] = state
special = {
    "source_sha256": PIN,
    "shaped": sorted(native_shaped, key=lambda row: row["recipe"]["identifier"]),
    "shapeless": sorted(native_shapeless, key=lambda row: row["identifier"]),
    "user_data_shapeless": sorted(builtins, key=lambda row: row["identifier"]),
    "multi": sorted(row["multi_recipe_uuid"] for row in capture["multi_recipes"]),
    "result_block_states": dict(sorted(result_states.items())),
    "result_user_data": dict(sorted(result_user_data.items())),
}
Path("data/measured/builtin_recipes.json").write_text(
    json.dumps(special, separators=(",", ":")) + "\n", encoding="utf-8", newline="\n",
)
print(f"Projected {len(native_shaped)} shaped, {len(native_shapeless)} shapeless, "
      f"{len(builtins)} user-data and {len(special['multi'])} multi recipes")
