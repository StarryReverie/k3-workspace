#!/usr/bin/env python3
"""Upload a K3 FIT image and boot it from U-Boot over TFTP.

The script connects with ostool, stops autoboot, uploads the image into
the current board session, initializes the board network with the known
BPI-SM10 sequence, then runs `tftpboot` and `bootm`. It waits for a
marker string (default: K3_BOOT_OK) and stores the full transcript.

It does not run saveenv, mmc write, reset, or net list.

Usage:
    ./scripts/alien/boot-k3-board.py
    ./scripts/alien/boot-k3-board.py --image alien/out-k3-diag/alien-k3-diag.itb
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
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parent.parent.parent
ANSI = re.compile(rb"\x1b\[[0-9;?]*[A-Za-z]")
SESSION_RE = re.compile(r"session_id:\s*([A-Za-z0-9_-]+)")
TFTP_RE = re.compile(r"tftp_url:\s*(\S+)")

NET_INIT = [
    ('test -n "$ethaddr" || setenv ethaddr $eth1addr', 1.0),
    ("setexpr.l sm10_dline *0xd4282bf0", 1.0),
    ('setexpr sm10_dline $sm10_dline "|" 0x10001', 1.0),
    ("mw.l 0xd4282bf0 $sm10_dline 1", 1.0),
    ("setenv netretry no && setenv autoload no && dhcp && setenv autoload yes", 7.0),
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--board",
        default=os.environ.get("BOARD_TYPE", "BananaPi"),
        help="board type passed to `ostool board connect -b` (default: %(default)s)",
    )
    parser.add_argument(
        "--image",
        type=Path,
        default=ROOT / "alien" / "out-k3-diag" / "alien-k3-diag.itb",
        help="local FIT image (default: %(default)s)",
    )
    parser.add_argument(
        "--remote",
        default=None,
        help="remote session path (default: boot/<image name>)",
    )
    parser.add_argument(
        "--load-addr",
        default="0x140000000",
        help="FIT load address (default: %(default)s)",
    )
    parser.add_argument(
        "--fdt",
        default="$fdtcontroladdr",
        help="FDT address passed to bootm; set to empty to omit (default: %(default)s)",
    )
    parser.add_argument(
        "--marker",
        default="K3_BOOT_OK",
        help="marker to wait for after bootm (default: %(default)s)",
    )
    parser.add_argument(
        "--boot-timeout",
        type=int,
        default=120,
        help="seconds to wait for the marker after bootm (default: %(default)s)",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=300,
        help="overall timeout in seconds (default: %(default)s)",
    )
    parser.add_argument(
        "--log",
        type=Path,
        default=None,
        help="transcript path (default: logs/boot-<timestamp>.log)",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    image = args.image.resolve()
    if not image.is_file():
        print(f"error: image not found: {image}", file=sys.stderr)
        return 2

    ostool = shutil.which("ostool")
    if not ostool:
        print("error: ostool not found; enter the dev shell first", file=sys.stderr)
        return 2

    remote = args.remote or f"boot/{image.name}"
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    log_path = args.log or ROOT / "logs" / f"boot-{stamp}.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    upload_log = log_path.with_suffix(".upload.log")

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
    upload_started = False
    stage = "net"
    path = ""
    command_index = 0
    next_command_at = 0.0
    marker_seen = False
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

    def upload() -> tuple[bool, str]:
        text = bytes(raw)
        match = SESSION_RE.search(text.decode("utf-8", "replace"))
        if not match:
            print("error: session id not found in board output", file=sys.stderr)
            return False, ""
        sid = match.group(1)
        print(f"session: {sid}", flush=True)

        # The session log is still growing; session-env.sh only needs the SID
        # and the token store, so parse the SID here and pass it explicitly.
        log_path.write_bytes(text)
        cmd = (
            f"source {ROOT / 'scripts' / 'session-env.sh'} {log_path} >/dev/null 2>&1; "
            f"SID={sid} {ROOT / 'scripts' / 'upload.sh'} {image} {remote}"
        )
        try:
            result = subprocess.run(
                ["bash", "-c", cmd],
                capture_output=True,
                text=True,
                env=env,
                timeout=120,
            )
        except subprocess.TimeoutExpired:
            print("error: upload timed out", file=sys.stderr)
            return False, ""

        upload_log.write_text(result.stdout + result.stderr)
        print(result.stdout, end="", flush=True)
        if result.returncode != 0:
            print(result.stderr, end="", file=sys.stderr, flush=True)
            return False, ""

        match = TFTP_RE.search(result.stdout)
        if match:
            parsed = urlparse(match.group(1))
            path = parsed.path.lstrip("/")
            print(f"tftp path: {path}", flush=True)
            return True, path

        fallback = f"ostool/sessions/{sid}/{remote}"
        print(f"tftp path (fallback): {fallback}", flush=True)
        return True, fallback

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

            if seen_prompt and not exit_sent:
                if not upload_started and now - prompt_at >= 1.0:
                    upload_started = True
                    ok, path = upload()
                    if not ok:
                        finish()
                    else:
                        stage = "net"
                        command_index = 0
                        next_command_at = time.monotonic() + 0.5
                elif upload_started and now >= next_command_at:
                    if stage == "net":
                        if command_index < len(NET_INIT):
                            cmd, delay = NET_INIT[command_index]
                            send(cmd.encode() + b"\r")
                            print(f"> {cmd}", flush=True)
                            command_index += 1
                            next_command_at = now + delay
                        else:
                            stage = "tftp"
                            next_command_at = now + 0.5
                    elif stage == "tftp":
                        cmd = f"tftpboot {args.load_addr} {path}"
                        send(cmd.encode() + b"\r")
                        print(f"> {cmd}", flush=True)
                        stage = "bootm_wait"
                        next_command_at = now + 12.0
                    elif stage == "bootm_wait":
                        if args.fdt:
                            cmd = f"bootm {args.load_addr} - {args.fdt}"
                        else:
                            cmd = f"bootm {args.load_addr}"
                        send(cmd.encode() + b"\r")
                        print(f"> {cmd}", flush=True)
                        stage = "wait"
                        next_command_at = now + args.boot_timeout
                    elif stage == "wait":
                        print("boot timeout without marker", flush=True)
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

            if args.marker.encode() in text:
                marker_seen = True
                print(f"marker seen: {args.marker}", flush=True)
                time.sleep(0.5)
                finish()

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
    stripped = ANSI.sub(b"", bytes(raw)).decode("utf-8", "replace")
    tail = stripped[-4000:]
    print(f"log   : {log_path}")
    print(f"prompt: {seen_prompt}")
    print(f"marker: {marker_seen}")
    print(f"exit  : {proc.returncode}")
    print("--- tail ---")
    print(tail)

    return 0 if marker_seen else 1


if __name__ == "__main__":
    raise SystemExit(main())
