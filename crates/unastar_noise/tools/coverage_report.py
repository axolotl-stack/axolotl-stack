#!/usr/bin/env python3
"""Registry coverage of native-compared Java frames.

Inputs are a pinned 1.21.11 server jar, the generated surface-rule sources of a
build (for `RULE_LEAVES`), and comparison results written by
`run_compare.py`-style runners: JSON lines with `label` and `expected`, plus the
`<actual>.coverage.json` sidecars written by `java_canonical_compare --generate`
built with `--features worldgen-coverage`.

Inventories are the registry entries reachable from each dimension: placed and
configured features through biome feature lists, structure pool feature
elements and feature-valued config fields; structures by biome; templates and
named processor lists through structure pools and their aliases, configured
features, and the template structures' fixed directories; carvers by biome;
surface-rule leaves of the dimension's noise settings. Entries reachable only
outside world generation (bonemeal, saplings, bonus chest, spawn platforms,
unreferenced templates) are listed separately rather than counted as gaps.

Usage: coverage_report.py <server.jar> <surface-out-dir> <results.jsonl>... --out <report.json>
"""
import argparse
import gzip
import json
import re
import zipfile
from collections import Counter
from pathlib import Path

DIMS = ("overworld", "the_nether", "the_end")
SURFACE_FILES = {"overworld": "overworld_surface.rs", "the_nether": "nether_surface.rs", "the_end": "end_surface.rs"}
END_BIOMES = {"minecraft:the_end", "minecraft:end_highlands", "minecraft:end_midlands",
              "minecraft:small_end_islands", "minecraft:end_barrens"}
# Template structures place templates from fixed directories, not pools.
TEMPLATE_DIRECTORIES = {"woodland_mansion": ("overworld",), "underwater_ruin": ("overworld",),
                        "shipwreck": ("overworld",), "igloo": ("overworld",), "end_city": ("the_end",),
                        "nether_fossils": ("the_nether",), "ruined_portal": ("overworld", "the_nether")}
FEATURE_KEYS = {"feature", "features", "default", "vegetation_feature", "placed_feature",
                "feature_true", "feature_false"}
EARLY_STATUSES = {"minecraft:empty", "minecraft:structure_starts", "minecraft:structure_references"}


class Jar:
    def __init__(self, path):
        self.zip = zipfile.ZipFile(path)
        self.names = self.zip.namelist()

    def json(self, path):
        return json.loads(self.zip.read(path))

    def registry(self, prefix, ext=".json"):
        return {"minecraft:" + n[len(prefix):-len(ext)]: n for n in self.names
                if n.startswith(prefix) and n.endswith(ext)}

    def tag(self, kind, name, seen=None):
        seen = set() if seen is None else seen
        path = f"data/minecraft/tags/{kind}/{name.split(':')[1]}.json"
        out = set()
        if path not in self.names:
            return out
        for value in self.json(path)["values"]:
            value = value if isinstance(value, str) else value["id"]
            if value.startswith("#"):
                if value not in seen:
                    seen.add(value)
                    out |= self.tag(kind, value[1:], seen)
            else:
                out.add(value)
        return out


def feature_refs(obj, out, key=None):
    """(kind, name) references: 'P' placed, 'C' configured."""
    if isinstance(obj, dict):
        if "placement" in obj and "feature" in obj:
            feature = obj["feature"]
            if isinstance(feature, str):
                out.append(("C", feature))
            else:
                feature_refs(feature, out)
            return out
        if isinstance(obj.get("type"), str) and "config" in obj:
            return feature_refs(obj["config"], out)
        for k, v in obj.items():
            if isinstance(v, str):
                if k in FEATURE_KEYS:
                    out.append(("P", v))
            else:
                feature_refs(v, out, k)
    elif isinstance(obj, list):
        for v in obj:
            if isinstance(v, str):
                if key in FEATURE_KEYS:
                    out.append(("P", v))
            else:
                feature_refs(v, out, key)
    return out


def inventories(jar, surface_dir):
    reg = {
        "placed_feature": jar.registry("data/minecraft/worldgen/placed_feature/"),
        "configured_feature": jar.registry("data/minecraft/worldgen/configured_feature/"),
        "structure": jar.registry("data/minecraft/worldgen/structure/"),
        "template_pool": jar.registry("data/minecraft/worldgen/template_pool/"),
        "processor_list": jar.registry("data/minecraft/worldgen/processor_list/"),
        "biome": jar.registry("data/minecraft/worldgen/biome/"),
        "template": jar.registry("data/minecraft/structure/", ".nbt"),
    }
    java_data = Path(__file__).resolve().parent.parent / "java_data"
    biomes = {
        "overworld": set(json.loads((java_data / "biome_parameters_overworld.json").read_text())["biomes"]),
        "the_nether": {b["biome"] for b in json.loads((java_data / "biome_parameters_nether.json").read_text())["biomes"]},
        "the_end": END_BIOMES,
    }
    structures = {d: set() for d in DIMS}
    for name, path in reg["structure"].items():
        spec = jar.json(path)["biomes"]
        found = (jar.tag("worldgen/biome", spec[1:]) if spec.startswith("#") else {spec}) \
            if isinstance(spec, str) else set(spec)
        for d in DIMS:
            if found & biomes[d]:
                structures[d].add(name)
    # Pools: members and every pool reachable from a structure's start pool,
    # through jigsaw blocks in member templates, fallbacks and pool aliases.
    members = {}
    for name, path in reg["template_pool"].items():
        pool = jar.json(path)
        found = []

        def elements(element):
            if element.get("element_type") == "minecraft:list_pool_element":
                for nested in element["elements"]:
                    yield from elements(nested)
            else:
                yield element
        for entry in pool["elements"]:
            for element in elements(entry["element"]):
                if "location" in element:
                    found.append(("T", element["location"]))
                if "feature" in element:
                    found.append(("P", element["feature"]))
                if isinstance(element.get("processors"), str):
                    found.append(("L", element["processors"]))
        members[name] = (found, pool.get("fallback"))
    template_pools = {}
    for name, path in reg["template"].items():
        raw = gzip.decompress(jar.zip.read(path))
        template_pools[name] = {m.decode() for m in re.findall(rb"minecraft:[a-z0-9_/]+", raw)
                                if m.decode() in members}
    pools = {}
    for name, path in reg["structure"].items():
        structure = jar.json(path)
        todo = [structure["start_pool"]] if "start_pool" in structure else []
        for alias in structure.get("pool_aliases", []):
            todo += [v for v in re.findall(r'"(minecraft:[a-z0-9_/]+)"', json.dumps(alias)) if v in members]
        seen = set()
        while todo:
            pool = todo.pop()
            if pool in seen or pool not in members:
                continue
            seen.add(pool)
            found, fallback = members[pool]
            if fallback:
                todo.append(fallback)
            for kind, value in found:
                if kind == "T":
                    todo += template_pools.get(value, ())
        pools[name] = seen

    def reach(seeds):
        placed, configured, todo, seen = set(), set(), list(seeds), set()
        while todo:
            kind, name = todo.pop()
            if (kind, name) in seen:
                continue
            seen.add((kind, name))
            if kind == "P" and name in reg["placed_feature"]:
                placed.add(name)
                feature = jar.json(reg["placed_feature"][name])["feature"]
                todo += [("C", feature)] if isinstance(feature, str) else feature_refs(feature, [])
            elif kind == "C" and name in reg["configured_feature"]:
                configured.add(name)
                todo += feature_refs(jar.json(reg["configured_feature"][name])["config"], [])
        return placed, configured

    inv = {}
    feature_templates = set()
    for name, path in reg["configured_feature"].items():
        feature_templates |= set(re.findall(r'"(minecraft:[a-z0-9_/]+)"', json.dumps(jar.json(path)))) & set(reg["template"])
    pooled_templates = set()
    for d in DIMS:
        seeds = [("P", f) for b in biomes[d] for step in jar.json(reg["biome"][b]).get("features", []) for f in step]
        seeds += [("P", v) for s in structures[d] for p in pools[s] for k, v in members[p][0] if k == "P"]
        placed, configured = reach(seeds)
        templates, lists = set(), set()
        for s in structures[d]:
            for p in pools[s]:
                for kind, value in members[p][0]:
                    if kind == "T":
                        templates.add(value)
                    elif kind == "L":
                        lists.add(value)
        pooled_templates |= templates
        for c in configured:
            templates |= set(re.findall(r'"(minecraft:[a-z0-9_/]+)"', json.dumps(jar.json(reg["configured_feature"][c])))) & set(reg["template"])
        carvers = set()
        for b in biomes[d]:
            c = jar.json(reg["biome"][b]).get("carvers", [])
            carvers |= {c} if isinstance(c, str) else set(c)
        text = (Path(surface_dir) / SURFACE_FILES[d]).read_text()
        leaves = re.findall(r'"([^"]*)"', re.search(r"RULE_LEAVES: \[&str; \d+usize\] = \[(.*?)\];", text, re.S).group(1))
        inv[d] = {"placed_feature": placed, "configured_feature": configured, "structure": structures[d],
                  "template": templates, "processor_list": lists, "carver": carvers,
                  "surface_rule": set(leaves), "biome": biomes[d]}
    unreferenced = []
    for name in reg["template"]:
        if name in pooled_templates or name in feature_templates:
            continue
        directory = name.split(":")[1].split("/")[0]
        if directory in TEMPLATE_DIRECTORIES:
            for d in TEMPLATE_DIRECTORIES[directory]:
                inv[d]["template"].add(name)
        else:
            unreferenced.append(name)
    excluded = {
        "placed_feature": sorted(set(reg["placed_feature"]) - set().union(*(inv[d]["placed_feature"] for d in DIMS))),
        "configured_feature": sorted(set(reg["configured_feature"]) - set().union(*(inv[d]["configured_feature"] for d in DIMS))),
        "template": sorted(unreferenced),
        "processor_list": sorted(set(reg["processor_list"]) - set().union(*(inv[d]["processor_list"] for d in DIMS))),
    }
    return inv, excluded


def dimension_of(label):
    lower = label.lower()
    if "the_end" in lower or "-end-" in lower:
        return "the_end"
    if "the_nether" in lower or "-nether-" in lower:
        return "the_nether"
    if "overworld" in lower:
        return "overworld"
    raise ValueError(f"no dimension in frame label {label}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("jar")
    parser.add_argument("surface_dir")
    parser.add_argument("results", nargs="+")
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    inv, excluded = inventories(Jar(args.jar), args.surface_dir)
    hits = {d: {} for d in DIMS}
    frames, voxels, differing = Counter(), Counter(), Counter()
    for results in args.results:
        directory = Path(results).parent
        for line in Path(results).read_text().splitlines():
            if not line.strip():
                continue
            row = json.loads(line)
            d = dimension_of(row["label"])
            report = row.get("report", {})
            frames[d] += 1
            voxels[d] += report.get("voxels", 0)
            for key in ("differing_states", "differing_quart_biomes", "differing_heightmap_columns",
                        "chunks_with_differing_block_entities"):
                differing[key] += report.get(key, 0)
            sidecar = directory / (row["label"] + ".actual.json.coverage.json")
            if not sidecar.exists():
                raise SystemExit(f"missing coverage sidecar {sidecar}")
            for kind, names in json.loads(sidecar.read_text()).items():
                hits[d].setdefault(kind, set()).update(names)
            expected = json.loads(Path(row["expected"]).read_text())
            for column in expected["chunks"]:
                if column["status"] not in EARLY_STATUSES:
                    hits[d].setdefault("biome", set()).update(
                        expected["biome_palette"][i] for i in set(column["biomes"]))
    coverage = {}
    for d in DIMS:
        coverage[d] = {}
        for kind, total in inv[d].items():
            found = hits[d].get(kind, set())
            coverage[d][kind] = {"exercised": len(found & total), "total": len(total),
                                 "missing": sorted(total - found), "outside_inventory": sorted(found - total)}
    Path(args.out).write_text(json.dumps({
        "frames_by_dimension": frames, "voxels_by_dimension": voxels, "differences": differing,
        "excluded_from_inventories": excluded, "coverage": coverage}, indent=1))
    for d in DIMS:
        print(d, {k: f"{v['exercised']}/{v['total']}" for k, v in coverage[d].items()})


if __name__ == "__main__":
    main()
