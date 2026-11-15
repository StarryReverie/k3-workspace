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
import threading
import time
from datetime import datetime
from pathlib import Path
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parent.parent.parent

def log(message: str) -> None:
    print(f"[{datetime.now():%H:%M:%S.%f}] {message}", flush=True)
ANSI = re.compile(rb"\x1b\[[0-9;?]*[A-Za-z]")
SESSION_RE = re.compile(r"session_id:\s*([A-Za-z0-9_-]+)")
TFTP_RE = re.compile(r"tftp_url:\s*(\S+)")

NET_INIT = [
    ('test -n "$ethaddr" || setenv ethaddr $eth1addr', 1.0),
    ("setexpr.l sm10_dline *0xd4282bf0", 1.0),
    ('setexpr sm10_dline $sm10_dline "|" 0x10001', 1.0),
    ("mw.l 0xd4282bf0 $sm10_dline 1", 1.0),
    ("setenv netretry no && setenv autoload no && dhcp && setenv autoload yes", 5.0),
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
        default=30,
        help="seconds to wait for the marker after bootm (default: %(default)s)",
    )
    parser.add_argument(
        "--pre-command",
        action="append",
        default=[],
        help="U-Boot command to send before upload; may be repeated",
    )
    parser.add_argument(
        "--post-command",
        action="append",
        default=[],
        help="command to send after the marker; may be repeated",
    )
    parser.add_argument(
        "--post-wait",
        type=float,
        default=2.0,
        help="seconds to wait after the last post command (default: %(default)s)",
    )
    parser.add_argument(
        "--sd-image",
        type=Path,
        default=None,
        help="image written to the SD card before booting",
    )
    parser.add_argument(
        "--sd-start",
        default="0x2000",
        help="SD write start sector in hex (default: %(default)s)",
    )
    parser.add_argument(
        "--sd-sectors",
        default=None,
        help="SD write sector count in hex; default is the image size / 512",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=240,
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

    sd_image = args.sd_image.resolve() if args.sd_image else None
    sd_sectors = args.sd_sectors
    if sd_image is not None:
        if not sd_image.is_file():
            print(f"error: SD image not found: {sd_image}", file=sys.stderr)
            return 2
        size = sd_image.stat().st_size
        if size % 512:
            print(f"error: SD image size {size} is not a multiple of 512", file=sys.stderr)
            return 2
        if sd_sectors is None:
            sd_sectors = f"0x{size // 512:x}"

    remote = args.remote or f"boot/{image.name}"
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    log(f"args: boot_timeout={args.boot_timeout} pre={args.pre_command} post={args.post_command}")
    log("connect start")

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
    pre_index = 0
    pre_next_at = 0.0
    upload_started = False
    upload_done = threading.Event()
    upload_result: list[object] = [False, ""]
    sd_upload_started = False
    sd_done = threading.Event()
    sd_result: list[object] = [False, ""]
    sd_path = ""
    sd_log = log_path.with_name(log_path.stem + ".sd.upload.log")
    SD_ADDR = "0x150000000"
    commands_started = False
    stage = "net"
    path = ""
    command_index = 0
    next_command_at = 0.0
    marker_seen = False
    post_index = 0
    post_next_at = 0.0
    exit_sent = False

    def send(data: bytes) -> None:
        os.write(master, data)

    def finish() -> None:
        nonlocal exit_sent
        if not exit_sent:
            log("finish")
            try:
                send(b"\x01x")
            except OSError:
                pass
            exit_sent = True

    def start_upload(
        local: Path, remote_name: str, result: list[object], done: threading.Event, out_log: Path
    ) -> bool:
        text = bytes(raw)
        match = SESSION_RE.search(text.decode("utf-8", "replace"))
        if not match:
            print("error: session id not found in board output", file=sys.stderr)
            return False
        sid = match.group(1)
        log(f"session: {sid}")

        # The session log is still growing; session-env.sh only needs the SID
        # and the token store, so parse the SID here and pass it explicitly.
        log_path.write_bytes(text)
        cmd = (
            f"source {ROOT / 'scripts' / 'session-env.sh'} {log_path} >/dev/null 2>&1; "
            f"SID={sid} {ROOT / 'scripts' / 'upload.sh'} {local} {remote_name}"
        )

        def worker() -> None:
            try:
                run = subprocess.run(
                    ["bash", "-c", cmd],
                    capture_output=True,
                    text=True,
                    env=env,
                    timeout=180,
                )
            except subprocess.TimeoutExpired:
                log("error: upload timed out")
                done.set()
                return

            out_log.write_text(run.stdout + run.stderr)
            print(run.stdout, end="", flush=True)
            if run.returncode != 0:
                print(run.stderr, end="", file=sys.stderr, flush=True)
                done.set()
                return

            found = TFTP_RE.search(run.stdout)
            if found:
                parsed = urlparse(found.group(1))
                tftp_path = parsed.path.lstrip("/")
                log(f"tftp path: {tftp_path}")
            else:
                tftp_path = f"ostool/sessions/{sid}/{remote_name}"
                log(f"tftp path (fallback): {tftp_path}")
            result[0] = True
            result[1] = tftp_path
            log("upload done")
            done.set()

        threading.Thread(target=worker, daemon=True).start()
        return True

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
                if pre_index < len(args.pre_command):
                    if pre_next_at == 0.0:
                        pre_next_at = now + 1.0
                    elif now >= pre_next_at:
                        cmd = args.pre_command[pre_index]
                        send(cmd.encode() + b"\r")
                        log(f"> {cmd}")
                        pre_index += 1
                        pre_next_at = now + 1.0
                        if pre_index == len(args.pre_command):
                            prompt_at = now
                elif not upload_started and now - prompt_at >= 1.0:
                    upload_started = True
                    if not start_upload(image, remote, upload_result, upload_done, upload_log):
                        finish()
                elif upload_started and not commands_started and upload_done.is_set():
                    ok = bool(upload_result[0])
                    path = str(upload_result[1])
                    if not ok:
                        finish()
                    elif sd_image is not None and not sd_upload_started:
                        sd_upload_started = True
                        if not start_upload(
                            sd_image, "boot/sdcard.img", sd_result, sd_done, sd_log
                        ):
                            finish()
                    elif sd_image is not None and not sd_done.is_set():
                        pass
                    else:
                        commands_started = True
                        if sd_image is not None:
                            if not sd_result[0]:
                                finish()
                            sd_path = str(sd_result[1])
                            stage = "sd_tftp"
                        else:
                            stage = "net"
                        command_index = 0
                        next_command_at = time.monotonic() + 0.5
                elif commands_started and now >= next_command_at:
                    if stage == "sd_tftp":
                        cmd = f"tftpboot {SD_ADDR} {sd_path}"
                        send(cmd.encode() + b"\r")
                        log(f"> {cmd}")
                        stage = "sd_write"
                        next_command_at = now + 6.0
                    elif stage == "sd_write":
                        cmd = f"mmc write {SD_ADDR} {args.sd_start} {sd_sectors}"
                        send(cmd.encode() + b"\r")
                        log(f"> {cmd}")
                        stage = "sd_verify"
                        next_command_at = now + 40.0
                    elif stage == "sd_verify":
                        cmd = f"mmc read {SD_ADDR} {args.sd_start} 1"
                        send(cmd.encode() + b"\r")
                        log(f"> {cmd}")
                        stage = "sd_dump"
                        next_command_at = now + 2.0
                    elif stage == "sd_dump":
                        cmd = f"md.l {SD_ADDR} 4"
                        send(cmd.encode() + b"\r")
                        log(f"> {cmd}")
                        stage = "net"
                        command_index = 0
                        next_command_at = now + 1.0
                    elif stage == "net":
                        if command_index < len(NET_INIT):
                            cmd, delay = NET_INIT[command_index]
                            send(cmd.encode() + b"\r")
                            log(f"> {cmd}")
                            command_index += 1
                            next_command_at = now + delay
                        else:
                            stage = "tftp"
                            next_command_at = now + 0.5
                    elif stage == "tftp":
                        cmd = f"tftpboot {args.load_addr} {path}"
                        send(cmd.encode() + b"\r")
                        log(f"> {cmd}")
                        stage = "bootm_wait"
                        next_command_at = now + 2.0
                    elif stage == "bootm_wait":
                        if args.fdt:
                            cmd = f"bootm {args.load_addr} - {args.fdt}"
                        else:
                            cmd = f"bootm {args.load_addr}"
                        send(cmd.encode() + b"\r")
                        log(f"> {cmd}")
                        stage = "wait"
                        next_command_at = now + args.boot_timeout
                    elif stage == "wait":
                        if not marker_seen:
                            log("boot timeout without marker")
                            finish()

            if marker_seen and not exit_sent and time.monotonic() >= post_next_at:
                if post_index < len(args.post_command):
                    cmd = args.post_command[post_index]
                    send(cmd.encode() + b"\r")
                    log(f"> {cmd}")
                    post_index += 1
                    post_next_at = time.monotonic() + 0.8
                else:
                    time.sleep(args.post_wait)
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
                log("U-Boot prompt seen")
                prompt_at = time.monotonic()
                interrupting = False

            if args.marker.encode() in text and not marker_seen and stage == "wait":
                marker_seen = True
                log(f"marker seen: {args.marker}")
                post_next_at = time.monotonic() + 0.5

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
