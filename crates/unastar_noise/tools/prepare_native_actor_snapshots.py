"""Prepare compact AOT actor data from NativeActorOracle's pinned observations.

Usage: python prepare_native_actor_snapshots.py <native-actor-saves.bin> <noise-crate>
The binary capture remains external; only native normalization deltas and minimal
fresh-constructor defaults are retained. No Java runtime is needed during builds.
"""
import hashlib
import io
import json
from pathlib import Path
import struct
import sys

PINS = {
    "server_sha1": "64bb6d763bed0a9f1d632ec347938594144943ed",
    "mappings_sha1": "5621e9253f05fd57872bbe7f8ddf5f9a7d525955",
    "code_sha256": "ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc",
}
FORMATS = {1: "b", 2: "h", 3: "i", 4: "q", 5: "I", 6: "Q", 7: "b", 11: "i", 12: "q"}


def number(stream, fmt):
    return struct.unpack(">" + fmt, stream.read(struct.calcsize(">" + fmt)))[0]


def string(stream):
    size = number(stream, "H")
    value = stream.read(size)
    if len(value) != size:
        raise ValueError("truncated native Java string")
    return value


def payload(stream, kind):
    if kind in range(1, 7):
        return kind, number(stream, FORMATS[kind])
    if kind == 8:
        return kind, string(stream)
    if kind == 9:
        element = number(stream, "B")
        return kind, (element, [payload(stream, element) for _ in range(number(stream, "i"))])
    if kind == 10:
        fields = {}
        while child := number(stream, "B"):
            name = string(stream)
            fields[name] = payload(stream, child)
        return kind, fields
    if kind in (7, 11, 12):
        return kind, [number(stream, FORMATS[kind]) for _ in range(number(stream, "i"))]
    raise ValueError(f"invalid native Java tag {kind}")


def root(data):
    stream = io.BytesIO(data)
    if number(stream, "B") != 10:
        raise ValueError("actor root must be Compound")
    string(stream)
    tag = payload(stream, 10)
    if stream.read():
        raise ValueError("trailing native Java NBT")
    return tag


def java_units(encoded):
    """Sort modified-UTF8 names in native UTF-16 order, without replacement."""
    units = []
    i = 0
    while i < len(encoded):
        b = encoded[i]
        i += 1
        if b < 128:
            units.append(b)
        elif b & 224 == 192:
            units.append(((b & 31) << 6) | (encoded[i] & 63))
            i += 1
        else:
            units.append(((b & 15) << 12) | ((encoded[i] & 63) << 6) | (encoded[i + 1] & 63))
            i += 2
    return units


def encode_payload(tag):
    kind, value = tag
    def enc_string(v):
        return struct.pack(">H", len(v)) + v
    if kind in range(1, 7):
        return struct.pack(">" + FORMATS[kind], value)
    if kind == 8:
        return enc_string(value)
    if kind == 9:
        element, values = value
        return bytes([element]) + struct.pack(">i", len(values)) + b"".join(encode_payload(v) for v in values)
    if kind == 10:
        return b"".join(bytes([v[0]]) + enc_string(k) + encode_payload(v)
                        for k, v in sorted(value.items(), key=lambda pair: java_units(pair[0]))) + b"\0"
    if kind in (7, 11, 12):
        return struct.pack(">i", len(value)) + b"".join(struct.pack(">" + FORMATS[kind], v) for v in value)
    raise ValueError(f"invalid native Java tag {kind}")


def encode_root(tag):
    return b"\x0a\0\0" + encode_payload(tag)


def field(stream):
    size = number(stream, "I")
    data = stream.read(size)
    if len(data) != size:
        raise ValueError("truncated native actor record")
    return data


def prepare(capture, crate):
    raw = capture.read_bytes()
    stream = io.BytesIO(raw)
    if stream.read(8) != b"NATJAVA2":
        raise ValueError("requires the inner-code-pinned NATJAVA2 observation format")
    for name, expected in PINS.items():
        if string(stream).decode("ascii") != expected:
            raise ValueError(f"native actor {name} differs")
    patches, patch_indexes, actors, defaults, families = [], {}, {}, {}, {}
    changes = 0
    while stream.tell() < len(raw):
        name = field(stream).decode("utf-8")
        before, after = field(stream), field(stream)
        source, saved = root(before), root(after)
        if name.startswith("runtime/"):
            defaults[name[8:]] = after
        if not name.startswith("data/minecraft/structure/"):
            continue
        changed = {k: v for k, v in saved[1].items() if source[1].get(k) != v}
        removed = sorted((k for k in source[1] if k not in saved[1]), key=java_units)
        patch = (10, {b"set": (10, changed), b"remove": (9, (8, [(8, k) for k in removed]))})
        reconstructed = {k: v for k, v in source[1].items() if k not in removed}
        reconstructed.update(changed)
        if (10, reconstructed) != saved:
            raise ValueError(f"normalization failed for {name}")
        encoded = encode_root(patch)
        if encoded not in patch_indexes:
            patch_indexes[encoded] = len(patches)
            patches.append(list(encoded))
        actors[name] = [hashlib.sha256(encode_root(source)).hexdigest(), patch_indexes[encoded]]
        actor_id = saved[1][b"id"][1].decode("ascii")
        families[actor_id] = families.get(actor_id, 0) + 1
        changes += source != saved
    data = {"version": "1.21.11", **PINS, "patches": patches, "actors": actors}
    (crate / "java_data/native_actor_normalization.json").write_text(json.dumps(data, separators=(",", ":")) + "\n", encoding="utf-8")
    lines = ["// Native Java 1.21.11 fresh BlockEntityType.create/saveWithFullMetadata observations.",
             "// Generated by tools/NativeActorOracle.java; server SHA1 " + PINS["server_sha1"] + ".",
             "// Native code SHA256 " + PINS["code_sha256"] + ".",
             "// Spawner variants call the native setEntityId API before saving.",
             "pub fn runtime_default(block: &str) -> Option<&'static [u8]> {", "    Some(match block {"]
    for name, data in defaults.items():
        lines.append(f'        "{name}" => &[')
        for offset in range(0, len(data), 24):
            lines.append("            " + ", ".join(str(v) for v in data[offset:offset + 24]) + ",")
        lines.append("        ],")
    lines.extend(["        _ => return None,", "    })", "}", ""])
    (crate / "java_data/native_actor_defaults.rs").write_text("\n".join(lines), encoding="utf-8")
    report = {"version": "1.21.11", **PINS, "capture_sha256": hashlib.sha256(raw).hexdigest(),
              "template_actors": len(actors), "native_save_changed_actors": changes,
              "normalization_patches": len(patches), "reconstruction_differences": 0,
              "runtime_constructor_and_set_entity_defaults": len(defaults), "families": families}
    capture.with_suffix(".json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, sort_keys=True))


if __name__ == "__main__":
    prepare(Path(sys.argv[1]), Path(sys.argv[2]))
