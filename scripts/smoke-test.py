#!/usr/bin/env python3
"""Automated K3 / BananaPi U-Boot smoke test.

What it does:
  1. opens `ostool board connect` in a real pty;
  2. waits for the U-Boot banner, then sends Ctrl+C to interrupt autoboot;
  3. waits for the `=>` prompt, runs `version` and `mmc list`;
  4. exits cleanly with Ctrl+A x;
  5. writes the full transcript to logs/smoke-<timestamp>.log.

Exit code 0 only if the prompt and `mmc list` output were observed.

Usage:
    ./scripts/smoke-test.py
    ./scripts/smoke-test.py --board BananaPi --timeout 120
"""
from __future__ import annotations

import argparse
import fcntl
import os
import pty
import re
import select
import shutil
import struct
import subprocess
import sys
import termios
import time
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ANSI = re.compile(rb"\x1b\[[0-9;?]*[A-Za-z]")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--board",
        default=os.environ.get("BOARD_TYPE", "BananaPi"),
        help="board type passed to `ostool board connect -b` (default: %(default)s)",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=120,
        help="overall timeout in seconds (default: %(default)s)",
    )
    parser.add_argument(
        "--log",
        type=Path,
        default=None,
        help="log path (default: logs/smoke-<timestamp>.log)",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    ostool = shutil.which("ostool")
    if not ostool:
        print("error: ostool not found; enter the dev shell first: nix develop", file=sys.stderr)
        return 2

    log_path = args.log or ROOT / "logs" / f"smoke-{datetime.now():%Y%m%d-%H%M%S}.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)

    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))

    env = dict(os.environ)
    env.setdefault("TERM", "xterm-256color")

    proc = subprocess.Popen(
        [ostool, "board", "connect", "-b", args.board],
        stdin=slave,
        stdout=slave,
        stderr=slave,
        env=env,
        start_new_session=True,
        close_fds=True,
    )
    os.close(slave)

    raw = bytearray()
    plain = bytearray()
    start = time.monotonic()

    seen_prompt = False
    prompt_at = 0.0
    interrupting = False
    last_interrupt = 0.0
    sent_version = False
    sent_mmc = False
    exit_sent = False
    failed_boot = False
    version_ok = False
    mmc_ok = False

    def send(data: bytes) -> None:
        os.write(master, data)

    try:
        while time.monotonic() - start < args.timeout:
            if proc.poll() is not None:
                break

            now = time.monotonic()

            # Only start sending Ctrl+C after U-Boot output is visible. Sending
            # it before ostool puts the pty in raw mode would deliver SIGINT to
            # the client process instead of forwarding it to the board.
            if interrupting and not seen_prompt and now - last_interrupt >= 0.3:
                try:
                    send(b"\x03")
                except OSError:
                    break
                last_interrupt = now

            if seen_prompt:
                if not sent_version and now - prompt_at >= 0.8:
                    send(b"version\r")
                    sent_version = True
                elif sent_version and not sent_mmc and now - prompt_at >= 2.8:
                    send(b"mmc list\r")
                    sent_mmc = True
                elif sent_mmc and not exit_sent and now - prompt_at >= 5.0:
                    send(b"\x01x")
                    exit_sent = True

            if failed_boot and not exit_sent and now - prompt_at >= 2.0:
                send(b"\x01x")
                exit_sent = True

            ready, _, _ = select.select([master], [], [], 0.2)
            if not ready:
                continue

            try:
                chunk = os.read(master, 65536)
            except OSError:
                break
            if not chunk:
                break

            raw.extend(chunk)
            plain.extend(ANSI.sub(b"", chunk))
            text = bytes(plain)

            if not seen_prompt and (b"U-Boot 2022.10 (" in text or b"Autoboot in" in text):
                interrupting = True

            if b"Starting kernel" in text and not seen_prompt:
                interrupting = False
                if not failed_boot:
                    failed_boot = True
                    prompt_at = time.monotonic()

            if not seen_prompt and b"=>" in text:
                seen_prompt = True
                prompt_at = time.monotonic()
                interrupting = False

            if seen_prompt and b"GNU ld" in text:
                version_ok = True
            if seen_prompt and b"sdh@" in text:
                mmc_ok = True

            if exit_sent and proc.poll() is not None:
                break
    finally:
        if not exit_sent:
            try:
                send(b"\x01x")
            except OSError:
                pass
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        os.close(master)

    log_path.write_bytes(bytes(raw))

    tail = ANSI.sub(b"", bytes(raw[-3000:])).decode("utf-8", "replace")
    print(f"log        : {log_path}")
    print(f"prompt =>  : {seen_prompt}")
    print(f"version ok : {version_ok}")
    print(f"mmc list ok: {mmc_ok}")
    print(f"exit code  : {proc.returncode}")
    print("--- tail ---")
    print(tail)

    return 0 if seen_prompt and mmc_ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
