#!/usr/bin/env python3
"""Connect to a K3 board, stop U-Boot autoboot, and run read-only commands.

The full pty transcript is written under logs/. The script does not call
saveenv, mmc write, reset, or net list.

Usage:
    ./scripts/uboot-probe.py
    ./scripts/uboot-probe.py --command "version" --command "bdinfo"
    ./scripts/uboot-probe.py --log logs/probe.log --command-delay 2.0
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

DEFAULT_COMMANDS = [
    "version",
    "bdinfo",
    "printenv fdtcontroladdr",
    "printenv fdtfile",
    "fdt addr $fdtcontroladdr",
    "fdt print /cpus",
    "fdt list /soc",
    "fdt print /memory",
]


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
        default=150,
        help="overall timeout in seconds (default: %(default)s)",
    )
    parser.add_argument(
        "--command",
        action="append",
        dest="commands",
        help="U-Boot command to run; may be repeated (default: built-in probe set)",
    )
    parser.add_argument(
        "--command-delay",
        type=float,
        default=2.0,
        help="seconds between commands (default: %(default)s)",
    )
    parser.add_argument(
        "--final-wait",
        type=float,
        default=3.0,
        help="seconds to wait after the last command (default: %(default)s)",
    )
    parser.add_argument(
        "--log",
        type=Path,
        default=None,
        help="log path (default: logs/probe-<timestamp>.log)",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    commands = args.commands or DEFAULT_COMMANDS
    ostool = shutil.which("ostool")
    if not ostool:
        print("error: ostool not found; enter the dev shell first", file=sys.stderr)
        return 2

    log_path = args.log or ROOT / "logs" / f"probe-{datetime.now():%Y%m%d-%H%M%S}.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)

    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 60, 160, 0, 0))

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

    interrupting = False
    last_interrupt = 0.0
    seen_prompt = False
    prompt_at = 0.0
    next_command_at = 0.0
    command_index = 0
    exit_sent = False

    def send(data: bytes) -> None:
        os.write(master, data)

    def finish() -> None:
        nonlocal exit_sent
        if not exit_sent:
            try:
                send(b"\x01x")
            except OSError:
                pass
            exit_sent = True

    try:
        while time.monotonic() - start < args.timeout:
            if proc.poll() is not None:
                break

            now = time.monotonic()

            if interrupting and not seen_prompt and now - last_interrupt >= 0.3:
                try:
                    send(b"\x03")
                except OSError:
                    break
                last_interrupt = now

            if seen_prompt and not exit_sent and now >= next_command_at:
                if command_index < len(commands):
                    cmd = commands[command_index]
                    send(cmd.encode() + b"\r")
                    print(f"> {cmd}", flush=True)
                    command_index += 1
                    delay = args.command_delay if command_index < len(commands) else args.final_wait
                    next_command_at = now + delay
                else:
                    finish()

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

            if not seen_prompt and b"=>" in text:
                seen_prompt = True
                prompt_at = time.monotonic()
                interrupting = False
                next_command_at = prompt_at + 0.8

            if exit_sent and proc.poll() is not None:
                break
    finally:
        finish()
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
    tail = ANSI.sub(b"", bytes(raw[-4000:])).decode("utf-8", "replace")
    print(f"log   : {log_path}")
    print(f"prompt: {seen_prompt}")
    print(f"exit  : {proc.returncode}")
    print("--- tail ---")
    print(tail)

    return 0 if seen_prompt else 1


if __name__ == "__main__":
    raise SystemExit(main())
