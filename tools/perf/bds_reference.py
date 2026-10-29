"""Chunk loading reference on BDS 1.26.51 with the same client as load_bench.

    python tools/perf/bds_reference.py --json tools/perf/results/<file>.json \
        [--radius 8 12 16] [--players 1] [--fly-seconds 20] [--dry-run]

Runs only on the shared runtime (C:/Users/jvigu/orca/bds-runtime/server)
under the native lock, on port 19140 (never 19132, where unastar is
playtested). It backs up server.properties, sets the benchmark world
(seed 12345, default terrain, view-distance 32, creative, client-side
generation off), and for every radius runs load_bench against a fresh world,
then restarts BDS and runs it again on the stored world. BDS is always
stopped and server.properties restored, and the benchmark world deleted,
even on failure.
"""

import argparse
import pathlib
import shutil
import subprocess
import sys
import threading
import time

SERVER = pathlib.Path("C:/Users/jvigu/orca/bds-runtime/server")
PROPERTIES = SERVER / "server.properties"
BACKUP = SERVER / "server.properties.perf-load-bench-backup"
LEVEL = "perf-load-bench"
PORT = 19140
OVERRIDES = {
    "server-port": str(PORT),
    "server-portv6": str(PORT + 1),
    "level-name": LEVEL,
    "level-seed": "12345",
    "level-type": "DEFAULT",
    "gamemode": "creative",
    "force-gamemode": "true",
    "difficulty": "easy",
    "max-players": "9",
    "view-distance": "32",
    "tick-distance": "4",
    "online-mode": "false",
    "client-side-chunk-generation-enabled": "false",
}


def write_properties():
    lines = []
    seen = set()
    for line in BACKUP.read_text(encoding="utf-8").splitlines():
        key = line.split("=", 1)[0].strip()
        if not line.startswith("#") and key in OVERRIDES:
            lines.append(f"{key}={OVERRIDES[key]}")
            seen.add(key)
        else:
            lines.append(line)
    lines += [f"{k}={v}" for k, v in OVERRIDES.items() if k not in seen]
    PROPERTIES.write_text("\n".join(lines) + "\n", encoding="utf-8")


class Bds:
    def __init__(self):
        self.process = subprocess.Popen(
            [str(SERVER / "bedrock_server.exe")],
            cwd=SERVER,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        self.started = threading.Event()
        threading.Thread(target=self._read, daemon=True).start()
        if not self.started.wait(180):
            self.stop()
            raise RuntimeError("BDS did not report 'Server started' within 180 s")

    def _read(self):
        for line in self.process.stdout:
            if "Server started" in line:
                self.started.set()
            if "ERROR" in line or "IPv4 supported, port" in line:
                print("bds:", line.rstrip(), file=sys.stderr)

    def stop(self):
        if self.process.poll() is None:
            try:
                self.process.stdin.write("stop\n")
                self.process.stdin.flush()
                self.process.wait(60)
            except Exception:
                self.process.kill()
                self.process.wait()


def bench(args, radius, world, label):
    command = [
        args.bench,
        "--addr", f"127.0.0.1:{PORT}",
        "--label", label,
        "--generator", "bds",
        "--radius", str(radius),
        "--players", str(args.players),
        "--json", args.json,
    ]
    if world == "fresh" and args.fly_seconds:
        command += ["--fly-seconds", str(args.fly_seconds)]
    print("==", " ".join(command), file=sys.stderr)
    subprocess.run(command, check=True, stdout=subprocess.DEVNULL, timeout=600)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", required=True)
    parser.add_argument("--radius", type=int, nargs="+", default=[8, 12, 16])
    parser.add_argument("--players", type=int, default=1)
    parser.add_argument("--fly-seconds", type=float, default=0)
    parser.add_argument("--bench", default="target/release/examples/load_bench.exe")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    running = subprocess.run(["tasklist"], capture_output=True, text=True).stdout
    if "bedrock_server" in running:
        sys.exit("another bedrock_server is running")
    if BACKUP.exists():
        sys.exit(f"{BACKUP} exists: restore it first")
    world = SERVER / "worlds" / LEVEL
    shutil.copy2(PROPERTIES, BACKUP)
    bds = None
    try:
        write_properties()
        radii = [4] if args.dry_run else args.radius
        for radius in radii:
            shutil.rmtree(world, ignore_errors=True)
            bds = Bds()
            bench(args, radius, "fresh", "bds-fresh")
            bds.stop()
            if args.dry_run:
                break
            bds = Bds()
            bench(args, radius, "generated", "bds-generated")
            bds.stop()
            bds = None
    finally:
        if bds is not None:
            bds.stop()
        shutil.copy2(BACKUP, PROPERTIES)
        BACKUP.unlink()
        shutil.rmtree(world, ignore_errors=True)
        time.sleep(1)


if __name__ == "__main__":
    main()
