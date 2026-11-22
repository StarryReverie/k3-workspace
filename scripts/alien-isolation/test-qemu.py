#!/usr/bin/env python3
"""Smoke-test the Alien isolation QEMU baseline.

Examples:
    ./scripts/alien-isolation/test-qemu.py --build
    ./scripts/alien-isolation/test-qemu.py --no-build

The test waits for the user `init` to exec `/bin/sh`, sends a marker
command, and saves the serial transcript under `logs/tests/`.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
ALIEN = ROOT / "alien-isolation"
BUILD = ROOT / "scripts" / "alien-isolation" / "build-qemu.sh"
RUN = ROOT / "scripts" / "alien-isolation" / "run-qemu.sh"
sys.path.insert(0, str(ROOT / "scripts" / "alien"))

from qemu_pty import QemuSession  # noqa: E402


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--build",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="rebuild images before running (default: %(default)s)",
    )
    parser.add_argument("--smp", type=int, default=2, help="number of harts")
    parser.add_argument("--timeout", type=float, default=90.0)
    parser.add_argument("--command-timeout", type=float, default=20.0)
    parser.add_argument(
        "--log-dir",
        type=Path,
        default=ROOT / "logs" / "tests",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.build:
        result = subprocess.run([str(BUILD)], cwd=ROOT)
        if result.returncode != 0:
            print("build failed", file=sys.stderr)
            return 1

    env = os.environ.copy()
    env["QEMU_TIMEOUT"] = "0"
    env["QEMU_SMP"] = str(args.smp)
    args.log_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    log_path = args.log_dir / f"{stamp}-alien-isolation-baseline.log"

    try:
        with QemuSession([str(RUN)], cwd=ROOT, env=env) as session:
            session.wait_for(r"Init process is running", args.timeout)
            session.wait_for(r'exec args: \["sh', args.timeout)
            session.send("echo M0_OK")
            session.wait_for(r"M0_OK", args.command_timeout)
            session.save_log(log_path, "scenario: qemu-baseline\nresult: PASS\n\n")
        print(f"PASS alien-isolation qemu baseline")
        print(f"log: {log_path}")
        return 0
    except Exception as error:  # noqa: BLE001
        print(f"FAIL: {error}", file=sys.stderr)
        print(f"log: {log_path}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
