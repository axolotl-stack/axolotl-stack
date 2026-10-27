"""Compare immutable native structure captures through the actual canonical CLI.

Only palette indices are remapped. Native states, actors, world options and
observation histories are retained; every source byte stream has SHA lineage.
"""
import argparse
import copy
import hashlib
import json
import pathlib
import re
import subprocess
import time


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def combine(corpus, run, entries):
    proof_path = corpus / run["context_proof"]
    assert digest(proof_path) == run["context_proof_sha256"]
    proof = load(proof_path)
    requests = proof["coordinate_request_order"]
    ranks = {coordinate: [] for coordinate in requests}
    for index, coordinate in enumerate(requests):
        ranks[coordinate].append(index)
    # Filenames contain the GLOBAL native request ordinal. Its rank within
    # this real dimension/stage group is the coordinate observation index.
    # Never mistake a CARVERS/Nether ordinal for an argument-list index.
    repeated = len(ranks) != len(requests)
    ordinal_ranks = {}
    if repeated:
        ordinals = []
        for entry in entries:
            suffix = re.search(r"-request(\d+)\.json$", entry["path"])
            assert suffix, (entry["path"], "repeated context lacks native request ordinal")
            ordinals.append((int(suffix[1]), entry))
        ordinals.sort(key=lambda row: row[0])
        assert len({ordinal for ordinal, _ in ordinals}) == len(entries)
        for rank, (_, entry) in enumerate(ordinals):
            assert ":".join(map(str, entry["chunk"])) == requests[rank]
            ordinal_ranks[entry["path"]] = rank
    seen = {}
    ranked = []
    for entry in entries:
        path = corpus / entry["path"]
        assert digest(path) == entry["sha256"], path
        cap = load(path)
        assert cap["coordinate_request_order"] == requests
        coordinate = ":".join(map(str, entry["chunk"]))
        rank = ordinal_ranks[entry["path"]] if repeated else ranks[coordinate][0]
        assert rank in ranks[coordinate], (path, rank, ranks[coordinate])
        assert rank not in seen, (path, "duplicate observation identity")
        seen[rank] = path
        if "coordinate_request_index" in cap:
            assert cap["coordinate_request_index"] == rank
        ranked.append((rank, entry, cap))
    assert set(seen) == set(range(len(requests))), (run["profile"], seen, requests)
    ranked.sort(key=lambda item: item[0])
    result = copy.deepcopy(ranked[0][2])
    result["palette"], result["biome_palette"], result["chunks"] = [], [], []
    states, biomes = {}, {}
    identity = ("edition", "version", "seed", "dimension", "state_encoding", "nbt_encoding", "min_y", "height", "structures", "datapacks", "inputs", "coordinate_request_order", "requested_stage")
    lineage = []
    for rank, entry, cap in ranked:
        assert all(cap.get(key) == result.get(key) for key in identity), entry["path"]
        state_indices, biome_indices = [], []
        for state in cap["palette"]:
            if state not in states:
                states[state] = len(result["palette"])
                result["palette"].append(state)
            state_indices.append(states[state])
        for biome in cap["biome_palette"]:
            if biome not in biomes:
                biomes[biome] = len(result["biome_palette"])
                result["biome_palette"].append(biome)
            biome_indices.append(biomes[biome])
        assert len(cap["chunks"]) == 1
        column = copy.deepcopy(cap["chunks"][0])
        assert [column["x"], column["z"]] == entry["chunk"]
        column["coordinate_request_index"] = rank
        column["blocks"][::2] = [state_indices[index] for index in column["blocks"][::2]]
        column["biomes"] = [biome_indices[index] for index in column["biomes"]]
        assert sum(column["blocks"][1::2]) == entry["canonical_states"]
        result["chunks"].append(column)
        lineage.append({"path": entry["path"], "sha256": entry["sha256"], "coordinate_request_index": rank})
    return result, lineage


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus", type=pathlib.Path)
    parser.add_argument("executable", type=pathlib.Path)
    parser.add_argument("output", type=pathlib.Path)
    parser.add_argument("--profile", action="append")
    parser.add_argument("--stage", action="append")
    args = parser.parse_args()
    # Constrain generation and inherited children to one CPU, without requiring
    # optional third-party modules on the comparison host.
    import ctypes
    if hasattr(ctypes, "windll"):
        kernel = ctypes.windll.kernel32
        kernel.GetCurrentProcess.restype = ctypes.c_void_p
        kernel.GetProcessAffinityMask.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(ctypes.c_size_t)]
        kernel.SetProcessAffinityMask.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        process_mask, system_mask = ctypes.c_size_t(), ctypes.c_size_t()
        handle = kernel.GetCurrentProcess()
        assert kernel.GetProcessAffinityMask(handle, ctypes.byref(process_mask), ctypes.byref(system_mask))
        mask = process_mask.value & -process_mask.value
        assert kernel.SetProcessAffinityMask(handle, ctypes.c_size_t(mask))
    args.output.mkdir(parents=True, exist_ok=True)
    manifest_path = args.corpus / "manifest.json"
    manifest = load(manifest_path)
    reports = {"executable": str(args.executable), "executable_sha256": digest(args.executable), "manifest_sha256": digest(manifest_path), "reports": []}
    stages = args.stage or ["SURFACE", "CARVERS", "FEATURES", "FEATURES_HALO_CLOSURE"]
    for stage in stages:
        for run in manifest["runs"]:
            if args.profile and run["profile"] not in args.profile:
                continue
            for dimension in ["minecraft:overworld", "minecraft:the_nether", "minecraft:the_end"]:
                entries = [entry for entry in manifest["files"] if entry["path"].replace("\\", "/").split("/")[0] == run["profile"] and entry["dimension"] == dimension and entry["stage"] == stage]
                if not entries:
                    continue
                expected, lineage = combine(args.corpus, run, entries)
                label = "-".join([run["profile"], dimension.split(":")[1], stage.lower()])
                native_path, actual_path = args.output / (label + "-native.json"), args.output / (label + "-actual.json")
                native_path.write_text(json.dumps(expected, separators=(",", ":")), encoding="utf-8")
                started = time.monotonic()
                generated = subprocess.run([str(args.executable), "--generate", str(native_path), str(actual_path)], capture_output=True, text=True, timeout=1800)
                report = {"label": label, "generate_exit": generated.returncode, "sources": lineage, "native_sha256": digest(native_path)}
                if generated.returncode == 0:
                    comparison = subprocess.run([str(args.executable), str(native_path), str(actual_path)], capture_output=True, text=True, timeout=180)
                    report.update(compare_exit=comparison.returncode, actual_sha256=digest(actual_path))
                    try:
                        report["comparison"] = json.loads(comparison.stdout)
                    except ValueError:
                        report.update(compare_stdout=comparison.stdout, compare_stderr=comparison.stderr)
                else:
                    report.update(generate_stdout=generated.stdout, generate_stderr=generated.stderr)
                report["elapsed_seconds"] = time.monotonic() - started
                reports["reports"].append(report)
                (args.output / "reports.json").write_text(json.dumps(reports, indent=2), encoding="utf-8")
                print(json.dumps({key: value for key, value in report.items() if key != "sources"}), flush=True)


if __name__ == "__main__":
    main()
