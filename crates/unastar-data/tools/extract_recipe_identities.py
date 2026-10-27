"""Extract legacy furnace identities from a pinned BDS CraftingData JSON capture.

Usage: python tools/extract_recipe_identities.py path/to/00054-CraftingDataPacket.json
Run from crates/unastar-data. The JSON is the bds-extractor debug projection;
its adjacent .bin must match the retained 1.26.51 manifest hash.
"""

import hashlib
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
for recipe in capture["shapeless_recipes"]:
    rows.setdefault(recipe["recipe_id"], []).append(recipe)


def record(table, legacy, name, data):
    if legacy in known_names and legacy == name:
        return
    value = {"name": name, "data": data}
    assert legacy not in table or table[legacy] == value, (legacy, table.get(legacy), value)
    table[legacy] = value


inputs, outputs = {}, {}
for path in sorted(Path("data/vanilla_bp/behavior_pack/recipes").rglob("*.json")):
    # Furnace documents in the pinned pack have no comments.
    document = json.loads(path.read_text(encoding="utf-8"))
    body = document.get("minecraft:recipe_furnace")
    if body is None:
        continue
    candidates = rows[body["description"]["identifier"]]
    wire = candidates[0]
    assert all(row["ingredients"] == wire["ingredients"] and row["results"] == wire["results"] for row in candidates)
    source = body["input"]
    if isinstance(source, str):
        ingredient = wire["ingredients"][0]
        names = [field["value"] for field in ingredient["descriptor"] if field["key"] == "name"]
        assert len(names) == 1, (path, ingredient)
        aux = ingredient["aux_value"]
        record(inputs, source, names[0], None if aux == 32767 else aux)
    result = body["output"]
    if isinstance(result, str):
        assert len(wire["results"]) == 1, path
        result_wire = wire["results"][0]
        record(outputs, result, by_id[result_wire["id"]], result_wire["auxvalue"])

result = {
    "source": "BDS 1.26.51 member/join/00054-CraftingDataPacket.bin",
    "source_sha256": PIN,
    "inputs": dict(sorted(inputs.items())),
    "outputs": dict(sorted(outputs.items())),
}
Path("data/measured/recipe_legacy_identities.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8", newline="\n")
print(f"Extracted {len(inputs)} input and {len(outputs)} output legacy identities")
