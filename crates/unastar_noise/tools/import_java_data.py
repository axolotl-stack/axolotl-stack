#!/usr/bin/env python3
"""Import the Java Edition reports and Java->Bedrock mappings used by the AOT toolchain.

Writes `crates/unastar_noise/java_data/` from:

* the data-generator output of the pinned Java server (`--reports --server`):
  `reports/blocks.json`, `reports/biome_parameters/minecraft/overworld.json`,
  `data/minecraft/tags/worldgen/biome/`;
* the data-generator `reports/blocks.json` of the Java version GeyserMC's mapping
  files are keyed by (their Java state ids index the mapping lists);
* GeyserMC/mappings (MIT) `blocks.nbt`, `collisions.nbt`, `biomes.json`;
* the Bedrock canonical block palette (`canonical_block_states.nbt`, network NBT)
  whose ordinals are unastar's runtime block ids.

Every pinned Java state must resolve to exactly one Bedrock canonical state, otherwise
the import fails. See `crates/unastar_noise/java_data/README.md` for the procedure.
"""

from __future__ import annotations

import argparse
import gzip
import json
import shutil
import struct
import sys
from pathlib import Path


class BigEndianNbt:
    """Minimal big-endian NBT reader (Java/Geyser files)."""

    def __init__(self, data: bytes) -> None:
        self.data = data
        self.pos = 0

    def take(self, n: int) -> bytes:
        out = self.data[self.pos : self.pos + n]
        self.pos += n
        return out

    def num(self, fmt: str):
        size = struct.calcsize(fmt)
        return struct.unpack(">" + fmt, self.take(size))[0]

    def string(self) -> str:
        return self.take(self.num("H")).decode("utf-8")

    def length(self) -> int:
        return self.num("i")

    def payload(self, tag: int):
        if tag == 1:
            return self.num("b")
        if tag == 2:
            return self.num("h")
        if tag == 3:
            return self.num("i")
        if tag == 4:
            return self.num("q")
        if tag == 5:
            return self.num("f")
        if tag == 6:
            return self.num("d")
        if tag == 7:
            return self.take(self.length())
        if tag == 8:
            return self.string()
        if tag == 9:
            inner = self.num("b")
            return [self.payload(inner) for _ in range(self.length())]
        if tag == 10:
            out = {}
            while True:
                inner = self.num("b")
                if inner == 0:
                    return out
                name = self.string()
                out[name] = self.payload(inner)
        if tag == 11:
            return [self.num("i") for _ in range(self.length())]
        if tag == 12:
            return [self.num("q") for _ in range(self.length())]
        raise ValueError(f"unknown NBT tag {tag}")

    def root(self):
        tag = self.num("b")
        self.string()
        return self.payload(tag)


class NetworkNbt(BigEndianNbt):
    """Little-endian network NBT (varint lengths, zigzag ints) used by Bedrock palettes."""

    def num(self, fmt: str):
        size = struct.calcsize(fmt)
        return struct.unpack("<" + fmt, self.take(size))[0]

    def varuint(self) -> int:
        result = shift = 0
        while True:
            byte = self.data[self.pos]
            self.pos += 1
            result |= (byte & 0x7F) << shift
            shift += 7
            if not byte & 0x80:
                return result

    def zigzag(self) -> int:
        value = self.varuint()
        return (value >> 1) ^ -(value & 1)

    def string(self) -> str:
        return self.take(self.varuint()).decode("utf-8")

    def length(self) -> int:
        return self.zigzag()

    def payload(self, tag: int):
        if tag in (3, 4):
            return self.zigzag()
        if tag in (11, 12):
            return [self.zigzag() for _ in range(self.length())]
        return super().payload(tag)


def read_nbt(path: Path):
    raw = path.read_bytes()
    if raw[:2] == b"\x1f\x8b":
        raw = gzip.decompress(raw)
    return BigEndianNbt(raw).root()


def read_canonical_palette(path: Path) -> list[dict]:
    reader = NetworkNbt(path.read_bytes())
    states = []
    while reader.pos < len(reader.data):
        states.append(reader.root())
    return states


def state_key(name: str, props: dict) -> str:
    return name + "|" + json.dumps(props, sort_keys=True)


def java_states(report: dict) -> list[tuple[str, dict]]:
    count = sum(len(block["states"]) for block in report.values())
    out: list[tuple[str, dict] | None] = [None] * count
    for name, block in report.items():
        for state in block["states"]:
            out[state["id"]] = (name, state.get("properties", {}))
    if any(entry is None for entry in out):
        raise SystemExit("Java block report has gaps in its state ids")
    return out  # type: ignore[return-value]


def property_order(name: str, block: dict) -> list[str]:
    """Property order whose cartesian product (last fastest) reproduces the state ids."""
    import itertools

    props = block.get("properties", {})
    keys = list(props)
    first = min(state["id"] for state in block["states"])
    for perm in itertools.permutations(keys):
        index = {
            combo: i
            for i, combo in enumerate(itertools.product(*[props[k] for k in perm]))
        }
        if all(
            state["id"] == first + index[tuple(state["properties"][k] for k in perm)]
            for state in block["states"]
        ):
            return list(perm)
    raise SystemExit(f"no property order reproduces the state ids of {name}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--java-datagen", type=Path, required=True)
    parser.add_argument("--java-version", required=True)
    parser.add_argument("--mapping-java-blocks", type=Path, required=True)
    parser.add_argument("--mapping-java-version", required=True)
    parser.add_argument("--geyser-dir", type=Path, required=True)
    parser.add_argument("--geyser-commit", required=True)
    parser.add_argument("--canonical-palette", type=Path, required=True)
    parser.add_argument("--bedrock-version", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    out: Path = args.out
    out.mkdir(parents=True, exist_ok=True)

    # Java block states (compact: the cartesian layout regenerates every id).
    report = json.loads((args.java_datagen / "reports" / "blocks.json").read_text())
    blocks = []
    for name, block in report.items():
        order = property_order(name, block)
        props = block.get("properties", {})
        default = next(s["id"] for s in block["states"] if s.get("default"))
        blocks.append(
            {
                "name": name,
                "first_id": min(s["id"] for s in block["states"]),
                "default_id": default,
                "properties": [[key, props[key]] for key in order],
            }
        )
    blocks.sort(key=lambda b: b["first_id"])
    (out / "blocks.json").write_text(
        json.dumps({"java_version": args.java_version, "blocks": blocks}, separators=(",", ":"))
        + "\n"
    )

    # Java -> Bedrock block states.
    pinned = java_states(report)
    mapping_report = json.loads(args.mapping_java_blocks.read_text())
    mapping_ids = {
        state_key(name, state.get("properties", {})): state["id"]
        for name, block in mapping_report.items()
        for state in block["states"]
    }
    geyser_blocks = read_nbt(args.geyser_dir / "blocks.nbt")["bedrock_mappings"]
    palette = read_canonical_palette(args.canonical_palette)
    canonical = {state_key(s["name"], s["states"]): i for i, s in enumerate(palette)}

    ordinals = []
    failures = []
    for java_id, (name, props) in enumerate(pinned):
        mapped = mapping_ids.get(state_key(name, props))
        if mapped is None:
            failures.append(f"{name}{props}: not in the {args.mapping_java_version} report")
            continue
        entry = geyser_blocks[mapped]
        bedrock_name = entry.get("bedrock_identifier", name)
        if ":" not in bedrock_name:
            bedrock_name = "minecraft:" + bedrock_name
        ordinal = canonical.get(state_key(bedrock_name, entry.get("state", {})))
        if ordinal is None:
            failures.append(f"{name}{props}: {bedrock_name}{entry.get('state', {})} not canonical")
            continue
        ordinals.append(ordinal)
    if failures:
        print("\n".join(failures[:50]), file=sys.stderr)
        raise SystemExit(f"{len(failures)} Java states do not map to Bedrock canonical states")

    (out / "bedrock_blocks.json").write_text(
        json.dumps(
            {
                "java_version": args.java_version,
                "bedrock_version": args.bedrock_version,
                "source": f"GeyserMC/mappings@{args.geyser_commit} (MIT), keyed by Java "
                f"{args.mapping_java_version} state ids",
                "bedrock_ordinals": ordinals,
            },
            separators=(",", ":"),
        )
        + "\n"
    )

    # Java -> Bedrock biome ids.
    biomes = json.loads((args.geyser_dir / "biomes.json").read_text())
    (out / "bedrock_biomes.json").write_text(
        json.dumps(
            {name: value["bedrock_id"] for name, value in sorted(biomes.items())}, indent=1
        )
        + "\n"
    )

    # Overworld biome parameter points; floats kept as the report's decimal strings so the
    # build script can parse them as f32 exactly like the Java float they came from.
    params_path = args.java_datagen / "reports" / "biome_parameters" / "minecraft" / "overworld.json"
    raw_points = json.loads(params_path.read_text(), parse_float=str, parse_int=str)["biomes"]
    names: list[str] = []
    points = []
    for point in raw_points:
        if point["biome"] not in names:
            names.append(point["biome"])
        p = point["parameters"]

        def pair(value):
            return value if isinstance(value, list) else [value, value]

        points.append(
            [names.index(point["biome"])]
            + pair(p["temperature"])
            + pair(p["humidity"])
            + pair(p["continentalness"])
            + pair(p["erosion"])
            + pair(p["depth"])
            + pair(p["weirdness"])
            + [p["offset"]]
        )
    (out / "biome_parameters_overworld.json").write_text(
        json.dumps(
            {
                "columns": "biome,temperature,humidity,continentalness,erosion,depth,weirdness"
                " as min,max pairs, then offset",
                "biomes": names,
                "points": points,
            },
            separators=(",", ":"),
        )
        + "\n"
    )

    # Biome and block tags, as the data generator writes them.
    for kind in (("worldgen", "biome"), ("block",)):
        tags_src = args.java_datagen / "data" / "minecraft" / "tags" / Path(*kind)
        tags_dst = out / "tags" / Path(*kind)
        if tags_dst.exists():
            shutil.rmtree(tags_dst)
        shutil.copytree(tags_src, tags_dst)

    print(f"{len(blocks)} blocks, {len(ordinals)} states, {len(points)} biome points -> {out}")


if __name__ == "__main__":
    main()
