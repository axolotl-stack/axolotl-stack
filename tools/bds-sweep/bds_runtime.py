"""Exclusive, loopback-only control of the shared BDS 1.26.51 runtime.

Used by the T1 capture runner (`capture.py`, plain BDS) and the T2 sweep
runner (`sweep.py`, BDS under Endstone). Rules this module enforces:

* one BDS per runtime: refuses to start when a `bedrock_server` process from
  this runtime's `server/` folder runs, or the game port is bound. Several
  runtimes (slots) can run side by side: each is a copy of the server folder
  with its own `BDS_RUNTIME` and `BDS_PORT` (see the README, "Parallel slots");
* `server.properties` is backed up before a session writes its own and is
  restored afterwards, also after a crash of a previous session (a stale
  backup is restored first);
* the session world is a harness-owned name (`level-name`), never the
  client-diff world;
* the server is stopped with the `stop` console command (graceful LevelDB
  close), killed only after a timeout.

Never launches the Minecraft client.
"""

from __future__ import annotations

import os
import queue
import re
import shutil
import subprocess
import threading
import time
from pathlib import Path

RUNTIME = Path(os.environ.get('BDS_RUNTIME', 'C:/Users/jvigu/orca/bds-runtime')).resolve()
SERVER = RUNTIME / 'server'
PYTHON = RUNTIME / 'venv' / 'Scripts' / 'python.exe'
# IPv4 game port of this runtime; IPv6 uses the next port. Slots use their own
# ports so they never collide with each other or with unastar on 19132.
PORT = int(os.environ.get('BDS_PORT', '19132'))
ADDR = f'127.0.0.1:{PORT}'
PROPERTIES = SERVER / 'server.properties'
BACKUP = SERVER / 'server.properties.bds-sweep-backup'
EXPECTED_VERSION = '26.51'
# Untracked caches shared by every worktree (Mojang-derived bulk data, raw
# sweep logs).
CAPTURES = RUNTIME / 'captures' / '1.26.51'
SWEEPS = RUNTIME / 'sweeps' / '1.26.51'


def bds_processes() -> list[str]:
    """Executable paths of running `bedrock_server` processes started from this
    runtime's `server/` folder. A process whose path cannot be read (another
    user's or an elevated one) counts as this runtime's, to stay safe."""
    script = ("Get-CimInstance Win32_Process -Filter \"Name='bedrock_server.exe'\" | "
              "ForEach-Object { if ($_.ExecutablePath) { $_.ExecutablePath } else { '?' } }")
    out = subprocess.run(['powershell', '-NoProfile', '-Command', script],
                         capture_output=True, text=True, check=True).stdout
    return [line.strip() for line in out.splitlines()
            if line.strip() and process_in_runtime(line.strip(), SERVER)]


def process_in_runtime(executable: str, server: Path) -> bool:
    """Whether a `bedrock_server` executable path belongs to `server` ('?' when
    the path is unreadable counts as belonging)."""
    if executable == '?':
        return True
    return Path(executable).resolve().parent == server.resolve()


def udp_port_bound(port: int) -> bool:
    """Whether a local UDP socket is bound to `port`. Only the local-address
    column counts: a client socket whose *remote* address is the port (a
    launcher pinging the server list) does not occupy it."""
    out = subprocess.run(['netstat', '-an', '-p', 'UDP'], capture_output=True, text=True, check=True).stdout
    return any(len(parts) >= 2 and parts[0] == 'UDP' and re.search(rf'[:.]{port}$', parts[1])
               for parts in map(str.split, out.splitlines()))


def restore_properties() -> None:
    """Puts back the properties a crashed session left replaced."""
    if BACKUP.exists():
        shutil.copyfile(BACKUP, PROPERTIES)
        BACKUP.unlink()


def render_properties(values: dict[str, object]) -> str:
    return ''.join(f'{key}={value}\n' for key, value in values.items())


class Bds:
    """One BDS session. `endstone=True` starts it under Endstone (plugins load),
    otherwise the plain `bedrock_server.exe` runs (vanilla commands only)."""

    def __init__(self, properties: dict[str, object], log: Path, *, endstone: bool,
                 env: dict[str, str] | None = None, startup_timeout: float = 240.0):
        self.properties = properties
        self.log_path = log
        self.endstone = endstone
        self.env = env or {}
        self.startup_timeout = startup_timeout
        self.process: subprocess.Popen[str] | None = None
        self.lines: queue.Queue[str] = queue.Queue()
        self.history: list[str] = []
        self._reader: threading.Thread | None = None

    @property
    def port(self) -> int:
        return int(self.properties.get('server-port', PORT))

    def __enter__(self) -> 'Bds':
        version = (SERVER / 'version.txt').read_text(encoding='utf-8').strip()
        if version != EXPECTED_VERSION:
            raise RuntimeError(f'shared runtime is BDS {version}, expected {EXPECTED_VERSION}')
        if bds_processes():
            raise RuntimeError(f'a bedrock_server process from {SERVER} is already running; one BDS per runtime')
        if udp_port_bound(self.port):
            raise RuntimeError(f'UDP port {self.port} is bound by another process')
        restore_properties()
        shutil.copyfile(PROPERTIES, BACKUP)
        PROPERTIES.write_text(render_properties(self.properties), encoding='utf-8')
        try:
            self._start()
            self.wait_for(r'Server started\.', self.startup_timeout)
        except BaseException:
            self.__exit__(None, None, None)
            raise
        return self

    def _start(self) -> None:
        env = {**os.environ, **self.env}
        if self.endstone:
            args = [str(PYTHON), '-m', 'endstone', '--server-folder', str(SERVER), '--no-interactive', '--yes']
        else:
            args = [str(SERVER / 'bedrock_server.exe')]
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        self.process = subprocess.Popen(args, cwd=SERVER, env=env, stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                        text=True, encoding='utf-8', errors='replace', bufsize=1)
        self._reader = threading.Thread(target=self._read, daemon=True)
        self._reader.start()

    def _read(self) -> None:
        assert self.process and self.process.stdout
        # Append: a resumed sweep keeps the log (and crash report) of the
        # previous attempt.
        with self.log_path.open('a', encoding='utf-8') as log:
            for line in self.process.stdout:
                log.write(line)
                log.flush()
                self.lines.put(line.rstrip('\n'))

    def wait_for(self, pattern: str, timeout: float) -> re.Match[str]:
        """Waits for a console line matching `pattern` (lines seen earlier
        are searched first)."""
        regex = re.compile(pattern)
        for line in self.history:
            if (match := regex.search(line)):
                return match
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(f'BDS console did not print /{pattern}/ within {timeout}s')
            try:
                line = self.lines.get(timeout=min(remaining, 0.5))
            except queue.Empty:
                if self.process and self.process.poll() is not None:
                    raise RuntimeError(f'BDS exited with {self.process.returncode} (log {self.log_path})')
                continue
            self.history.append(line)
            if (match := regex.search(line)):
                return match

    def command(self, line: str) -> None:
        assert self.process and self.process.stdin
        self.process.stdin.write(line + '\n')
        self.process.stdin.flush()

    def __exit__(self, *_exc) -> None:
        try:
            if self.process and self.process.poll() is None:
                try:
                    self.command('stop')
                    self.process.wait(timeout=60)
                except (OSError, subprocess.TimeoutExpired):
                    self.process.kill()
                    self.process.wait()
            if self._reader:
                self._reader.join(timeout=5)
        finally:
            restore_properties()


def base_properties(level_name: str, **overrides: object) -> dict[str, object]:
    """Loopback-only, offline session properties; everything not listed keeps
    the BDS default."""
    values: dict[str, object] = {
        'server-name': f'bds-sweep {level_name}',
        'server-port': PORT,
        'server-portv6': PORT + 1,
        'online-mode': 'false',
        'enable-lan-visibility': 'false',
        'allow-cheats': 'true',
        'level-name': level_name,
        'max-players': 2,
        'content-log-file-enabled': 'false',
    }
    values.update({key.replace('_', '-'): value for key, value in overrides.items()})
    return values
