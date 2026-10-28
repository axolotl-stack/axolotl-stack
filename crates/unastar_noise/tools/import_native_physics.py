#!/usr/bin/env python3
"""Import the pinned JavaBlockPhysics capture into the compile-time data model."""

import argparse
import hashlib
import json
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--capture", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    raw = args.capture.read_bytes()
    capture = json.loads(raw)
    if capture.get("schema") != "native-state-physics-v4" or capture.get("version") != "1.21.11":
        raise SystemExit("expected a pinned Java 1.21.11 native-state-physics-v4 capture")
    states = capture["states"]
    if not states or any(state["id"] != index for index, state in enumerate(states)):
        raise SystemExit("native state ids must be contiguous and in registry order")
    metadata = {"schema": capture["schema"], "java_version": capture["version"]}
    for field in ["server_sha1", "mappings_sha1", "inner_sha256", "source_sha256"]:
        metadata[field] = capture[field]
    metadata["native_capture_sha256"] = hashlib.sha256(raw).hexdigest()
    for field in ["flags", "wall_above_flags", "center_support_flags", "behavior_flags"]:
        metadata[f"{field}_sha256"] = hashlib.sha256(bytes(state[field] for state in states)).hexdigest()
    # Keep complete native canonical identities and every observed byte. Local
    # invocation paths/guard bookkeeping are not compile-time data dependencies.
    content = "{\n" + "".join(
        "  " + json.dumps(key) + ": " + json.dumps(value) + ",\n"
        for key, value in metadata.items()
    ) + '  "states": [\n' + ",\n".join(
        "    " + json.dumps(state, separators=(",", ":"), ensure_ascii=True)
        for state in states
    ) + "\n  ]\n}\n"
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(content, encoding="utf-8", newline="\n")
    print(json.dumps({"states": len(states), "sha256": hashlib.sha256(content.encode()).hexdigest()}))


if __name__ == "__main__":
    main()
