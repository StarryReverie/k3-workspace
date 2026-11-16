#!/usr/bin/env python3
"""Reusable PTY session for QEMU serial tests.

The session owns the QEMU process, accumulates serial output, strips ANSI
escape sequences for matching, and saves a cleaned transcript on exit.
"""

from __future__ import annotations

import os
import pty
import re
import select
import signal
import subprocess
import time
from pathlib import Path

ANSI = re.compile(rb"\x1b\[[0-9;?]*[A-Za-z]")
ANSI_OSC = re.compile(rb"\x1b\][^\x07]*(?:\x07|\x1b\\)")
ANSI_CHARSET = re.compile(rb"\x1b[()][A-Z0-9]")


class SessionError(RuntimeError):
    """The session ended or produced unexpected output."""


class SessionTimeout(SessionError):
    """A pattern was not observed before the deadline."""


class QemuSession:
    """Run QEMU on a PTY and interact through its serial console."""

    def __init__(
        self,
        argv: list[str],
        *,
        cwd: Path | None = None,
        env: dict[str, str] | None = None,
        verbose: bool = False,
    ) -> None:
        self.argv = [str(item) for item in argv]
        self.verbose = verbose
        self.master, slave = pty.openpty()
        self.process = subprocess.Popen(
            self.argv,
            stdin=slave,
            stdout=slave,
            stderr=slave,
            cwd=str(cwd) if cwd is not None else None,
            env=env,
            close_fds=True,
            start_new_session=True,
        )
        os.close(slave)
        self.raw = bytearray()
        self.closed = False

    def __enter__(self) -> "QemuSession":
        return self

    def __exit__(self, _exc_type, _exc, _tb) -> None:
        self.close()

    def text(self) -> str:
        """Return the current serial output without ANSI or carriage returns."""
        cleaned = ANSI.sub(b"", bytes(self.raw))
        cleaned = ANSI_OSC.sub(b"", cleaned)
        cleaned = ANSI_CHARSET.sub(b"", cleaned)
        return cleaned.decode("utf-8", "replace").replace("\r", "")

    def _read(self, timeout: float) -> bool:
        """Read one chunk. Return False when the process and PTY are finished."""
        try:
            readable, _, _ = select.select([self.master], [], [], timeout)
        except (OSError, ValueError):
            return False
        if not readable:
            return True
        try:
            data = os.read(self.master, 8192)
        except OSError:
            return False
        if not data:
            return False
        self.raw.extend(data)
        if self.verbose:
            os.write(1, data)
        return True

    def wait_for(self, pattern: str, timeout: float = 30.0) -> re.Match[str]:
        """Wait until a regular expression appears in the cleaned output."""
        regex = re.compile(pattern, re.MULTILINE)
        deadline = time.monotonic() + timeout
        while True:
            match = regex.search(self.text())
            if match:
                return match
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise SessionTimeout(
                    f"timeout waiting for {pattern!r}; last output:\n{self.tail()}"
                )
            if not self._read(min(0.2, remaining)):
                if self.process.poll() is not None:
                    raise SessionError(
                        f"QEMU exited with {self.process.returncode} while "
                        f"waiting for {pattern!r}; last output:\n{self.tail()}"
                    )
                # The PTY is closed but the process may still be reaping.
                time.sleep(0.05)

    def send(self, command: str) -> None:
        """Send one command line to the serial console."""
        if self.closed:
            raise SessionError("session is closed")
        os.write(self.master, command.encode("utf-8") + b"\n")

    def send_and_wait(
        self, command: str, pattern: str, timeout: float = 10.0
    ) -> re.Match[str]:
        self.send(command)
        return self.wait_for(pattern, timeout)

    def tail(self, lines: int = 30) -> str:
        content = self.text().splitlines()
        return "\n".join(content[-lines:])

    def save_log(self, path: Path, header: str = "") -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        body = self.text()
        path.write_text(header + body, encoding="utf-8")

    def close(self) -> None:
        if self.closed:
            return
        self.closed = True
        if self.process.poll() is None:
            try:
                os.write(self.master, b"\x01x")
            except OSError:
                pass
            time.sleep(0.2)
            if self.process.poll() is None:
                try:
                    os.killpg(self.process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    self.process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    try:
                        os.killpg(self.process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    self.process.wait(timeout=3)
        try:
            os.close(self.master)
        except OSError:
            pass
