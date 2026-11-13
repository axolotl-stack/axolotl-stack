"""Run a benchmark process and report its wall time and CPU time (Windows).

    python tools/perf/cpu_run.py [--mask 0xffff] [--json out.json --label L] -- <command...>

Prints the command's stdout, then one line `cpu_run: wall_ms=.. cpu_ms=.. peak_mb=..`.
CPU time is user + kernel time of the process (GetProcessTimes), so it
includes setup and teardown. `--mask` sets the processor affinity before the
process does any work (it starts suspended). With --json, appends
{label, command, mask, wall_ms, cpu_ms, peak_mb, stdout_json} to a JSON list.
"""

import ctypes
import json
import subprocess
import sys
import time
from ctypes import wintypes

kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
CREATE_SUSPENDED = 0x4
THREAD_SUSPEND_RESUME = 0x0002


class FILETIME(ctypes.Structure):
    _fields_ = [("low", wintypes.DWORD), ("high", wintypes.DWORD)]

    def seconds(self):
        return ((self.high << 32) | self.low) / 1e7


def cpu_seconds(handle):
    creation, exit_, kernel, user = FILETIME(), FILETIME(), FILETIME(), FILETIME()
    if not kernel32.GetProcessTimes(
        wintypes.HANDLE(handle),
        ctypes.byref(creation),
        ctypes.byref(exit_),
        ctypes.byref(kernel),
        ctypes.byref(user),
    ):
        raise ctypes.WinError(ctypes.get_last_error())
    return kernel.seconds() + user.seconds()


class PROCESS_MEMORY_COUNTERS(ctypes.Structure):
    _fields_ = [
        ("cb", wintypes.DWORD),
        ("PageFaultCount", wintypes.DWORD),
        ("PeakWorkingSetSize", ctypes.c_size_t),
        ("WorkingSetSize", ctypes.c_size_t),
        ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
        ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
        ("PagefileUsage", ctypes.c_size_t),
        ("PeakPagefileUsage", ctypes.c_size_t),
    ]


def peak_working_set(handle):
    counters = PROCESS_MEMORY_COUNTERS()
    counters.cb = ctypes.sizeof(counters)
    psapi = ctypes.WinDLL("psapi", use_last_error=True)
    if not psapi.GetProcessMemoryInfo(
        wintypes.HANDLE(handle), ctypes.byref(counters), counters.cb
    ):
        raise ctypes.WinError(ctypes.get_last_error())
    return counters.PeakWorkingSetSize


def resume_all_threads(pid):
    # Toolhelp snapshot of threads; resume those of the child.
    TH32CS_SNAPTHREAD = 0x4

    class THREADENTRY32(ctypes.Structure):
        _fields_ = [
            ("dwSize", wintypes.DWORD),
            ("cntUsage", wintypes.DWORD),
            ("th32ThreadID", wintypes.DWORD),
            ("th32OwnerProcessID", wintypes.DWORD),
            ("tpBasePri", wintypes.LONG),
            ("tpDeltaPri", wintypes.LONG),
            ("dwFlags", wintypes.DWORD),
        ]

    snapshot = kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
    entry = THREADENTRY32()
    entry.dwSize = ctypes.sizeof(THREADENTRY32)
    ok = kernel32.Thread32First(snapshot, ctypes.byref(entry))
    while ok:
        if entry.th32OwnerProcessID == pid:
            thread = kernel32.OpenThread(THREAD_SUSPEND_RESUME, False, entry.th32ThreadID)
            kernel32.ResumeThread(thread)
            kernel32.CloseHandle(thread)
        ok = kernel32.Thread32Next(snapshot, ctypes.byref(entry))
    kernel32.CloseHandle(snapshot)


def main():
    args = sys.argv[1:]
    mask, out, label = None, None, ""
    while args and args[0] != "--":
        flag = args.pop(0)
        if flag == "--mask":
            mask = int(args.pop(0), 16)
        elif flag == "--json":
            out = args.pop(0)
        elif flag == "--label":
            label = args.pop(0)
        else:
            sys.exit(f"unknown flag {flag}")
    command = args[1:]
    started = time.perf_counter()
    process = subprocess.Popen(
        command, stdout=subprocess.PIPE, creationflags=CREATE_SUSPENDED
    )
    if mask is not None:
        if not kernel32.SetProcessAffinityMask(wintypes.HANDLE(process._handle), mask):
            process.kill()
            raise ctypes.WinError(ctypes.get_last_error())
    resume_all_threads(process.pid)
    stdout, _ = process.communicate()
    wall_ms = (time.perf_counter() - started) * 1e3
    cpu_ms = cpu_seconds(process._handle) * 1e3
    peak_mb = peak_working_set(process._handle) / 2**20
    text = stdout.decode("utf-8", "replace")
    sys.stdout.write(text)
    print(
        f"cpu_run: exit={process.returncode} wall_ms={wall_ms:.1f} cpu_ms={cpu_ms:.1f} peak_mb={peak_mb:.0f}"
    )
    if out:
        try:
            parsed = json.loads(text)
        except ValueError:
            parsed = None
        try:
            runs = json.load(open(out, encoding="utf-8"))
        except (OSError, ValueError):
            runs = []
        runs.append(
            {
                "label": label,
                "command": command,
                "mask": None if mask is None else hex(mask),
                "exit": process.returncode,
                "wall_ms": wall_ms,
                "cpu_ms": cpu_ms,
                "peak_mb": peak_mb,
                "stdout_json": parsed,
            }
        )
        json.dump(runs, open(out, "w", encoding="utf-8"), indent=2)
    sys.exit(process.returncode)


if __name__ == "__main__":
    main()
