"""Serialize headless BDS commands using temporary scoreboard acknowledgments.

Vanilla BDS does not print /say announcements in the observed playerless session.
Objective add/remove responses include a unique name and need no player. Each
objective is removed before the command completes; the disposable world owns it.
"""
from __future__ import annotations

import queue
import re
import time
import uuid


class Console:
    def __init__(self, server, deadline: float):
        self.server = server
        self.deadline = deadline

    def receive(self, objective: str, expected: str, timeout: float, result: dict, collect: bool) -> None:
        deadline = min(self.deadline, time.monotonic() + timeout)
        pattern = re.compile(re.escape(expected), re.IGNORECASE)
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(f"console synchronization timed out waiting for {expected!r}")
            try:
                line = self.server.lines.get(timeout=min(remaining, 0.5))
            except queue.Empty:
                if self.server.process and self.server.process.poll() is not None:
                    raise RuntimeError(f"BDS exited with code {self.server.process.returncode}")
                continue
            self.server.history.append(line)
            if objective in line and " ERROR]" in line:
                raise RuntimeError(f"console barrier failed: {line}")
            if pattern.search(line):
                return
            if collect:
                result["response_lines"].append(line)
                result["response"] = "\n".join(result["response_lines"])

    def command(self, result: dict, timeout: float) -> dict:
        result.update(status="running", response="", response_lines=[])
        objective = f"wgc_{uuid.uuid4().hex[:12]}"
        begin = f"scoreboard objectives add {objective} dummy"
        end = f"scoreboard objectives remove {objective}"
        result["barriers"] = {"objective": objective, "begin_command": begin, "end_command": end}
        try:
            if time.monotonic() >= self.deadline:
                raise TimeoutError("exclusive slot time budget exhausted")
            self.server.command(begin)
            self.receive(objective, f"Added new objective '{objective}' successfully", timeout, result, False)
            self.server.command(result["command"])
            self.server.command(end)
            self.receive(objective, f"Removed objective '{objective}' successfully", timeout, result, True)
            result["status"] = "received"
        except BaseException as exc:
            result.update(status="failed", error=f"{type(exc).__name__}: {exc}")
            raise
        return result
