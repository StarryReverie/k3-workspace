#!/usr/bin/env python3
"""Run the Alien isolation QEMU regression scenarios.

Scenarios:
    baseline   two harts, boot to shell
    smp-1      one hart
    smp-2      two harts
    smp-4      four harts
    smp-8      eight harts
    disk-load  two harts, load the vfs2 domain from the FAT disk with dvfs
    all        smp-1, smp-2, smp-4, smp-8, disk-load

Each scenario waits for the user init to exec /bin/sh, checks every secondary
hart reported `hart N start...`, confirms the shell with a retried round trip,
sends a marker command, and requires the marker on its own line. Images are
built with LOG=WARN by default so that the per-syscall INFO traffic does not
bury console output.

Examples:
    ./scripts/alien-isolation/test-qemu.py --build --list
    ./scripts/alien-isolation/test-qemu.py --build all
    ./scripts/alien-isolation/test-qemu.py --no-build smp-8
    ./scripts/alien-isolation/test-qemu.py --build disk-load
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import time
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
ALIEN = ROOT / "alien-isolation"
BUILD = ROOT / "scripts" / "alien-isolation" / "build-qemu.sh"
RUN = ROOT / "scripts" / "alien-isolation" / "run-qemu.sh"
sys.path.insert(0, str(ROOT / "scripts" / "alien"))

from qemu_pty import QemuSession, SessionTimeout  # noqa: E402


@dataclass(frozen=True)
class Scenario:
    name: str
    smp: int
    disk_load: bool = False
    marker: str = "M1_OK"

    @property
    def title(self) -> str:
        kind = "disk-load" if self.disk_load else "shell"
        return f"{self.name} (smp={self.smp}, {kind})"


SCENARIOS = {
    "baseline": Scenario("baseline", 2),
    "smp-1": Scenario("smp-1", 1, marker="M1_SMP1_OK"),
    "smp-2": Scenario("smp-2", 2, marker="M1_SMP2_OK"),
    "smp-4": Scenario("smp-4", 4, marker="M1_SMP4_OK"),
    "smp-8": Scenario("smp-8", 8, marker="M1_SMP8_OK"),
    "disk-load": Scenario("disk-load", 2, disk_load=True, marker="M1_DISK_OK"),
}
ALIASES = {
    "all": ["smp-1", "smp-2", "smp-4", "smp-8", "disk-load"],
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "scenarios",
        nargs="*",
        default=["baseline"],
        help="scenario names (default: baseline)",
    )
    parser.add_argument("--list", action="store_true", help="list scenarios")
    parser.add_argument(
        "--build",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="rebuild images before running (default: %(default)s)",
    )
    parser.add_argument("--log-level", default="WARN", help="domain log level")
    parser.add_argument("--timeout", type=float, default=180.0, help="boot timeout")
    parser.add_argument(
        "--command-timeout", type=float, default=30.0, help="console command timeout"
    )
    parser.add_argument(
        "--log-dir",
        type=Path,
        default=ROOT / "logs" / "tests",
    )
    return parser.parse_args()


def resolve(names: list[str]) -> list[Scenario]:
    resolved: list[Scenario] = []
    for name in names:
        if name in ALIASES:
            resolved.extend(SCENARIOS[item] for item in ALIASES[name])
        elif name in SCENARIOS:
            resolved.append(SCENARIOS[name])
        else:
            raise SystemExit(f"unknown scenario: {name}")
    return resolved


def image_dir(smp: int) -> Path:
    return ALIEN / "build" / f"smp-{smp}"


def image_ok(smp: int, log_level: str) -> bool:
    out = image_dir(smp)
    if not all(
        (out / name).is_file()
        for name in ("kernel", "initramfs.cpio.gz", "sdcard.img")
    ):
        return False
    info = out / "build-info"
    if not info.is_file():
        return False
    values = dict(
        line.split("=", 1)
        for line in info.read_text().splitlines()
        if "=" in line
    )
    return values.get("SMP") == str(smp) and values.get("LOG") == log_level


def wait_for_shell(session: QemuSession, timeout: float) -> None:
    """Confirm the shell reads input and writes output.

    The user init prints `Init process is running` through the VFS path.
    Under eight harts that line can be lost even though the shell is ready.
    A retried round trip through the console is a stronger readiness check.
    """
    marker = "__M1_READY__"
    deadline = time.monotonic() + timeout
    while True:
        session.send(f"echo {marker}")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise SessionTimeout("shell did not answer the readiness probe")
        try:
            session.wait_for(rf"(?m)^{marker}\s*$", min(2.0, remaining))
            return
        except SessionTimeout:
            time.sleep(0.5)


def build_image(smp: int, log_level: str) -> None:
    command = [
        str(BUILD),
        "--smp",
        str(smp),
        "--log",
        log_level,
        "--out",
        str(image_dir(smp)),
    ]
    print(f"build: smp={smp} log={log_level}")
    result = subprocess.run(command, cwd=ROOT)
    if result.returncode != 0:
        raise SystemExit(f"build failed for smp={smp}")


def run_scenario(
    scenario: Scenario, args: argparse.Namespace, log_path: Path
) -> None:
    out = image_dir(scenario.smp)
    env = os.environ.copy()
    env.update(
        {
            "QEMU_TIMEOUT": "0",
            "QEMU_SMP": str(scenario.smp),
            "OUT_DIR": str(out),
        }
    )
    header = (
        f"scenario: {scenario.name}\n"
        f"smp: {scenario.smp}\n"
        f"disk-load: {scenario.disk_load}\n"
        f"log-level: {args.log_level}\n"
    )
    with QemuSession(
        [str(RUN), "--smp", str(scenario.smp), "--out", str(out), "--timeout", "0"],
        cwd=ROOT,
        env=env,
    ) as session:
        try:
            session.wait_for(r'exec args: \["sh', args.timeout)
            boot = session.wait_for(r"Boot hart (\d+)", args.timeout)
            boot_hart = int(boot.group(1))
            expected = set(range(scenario.smp))
            expected.discard(boot_hart)
            for hart in sorted(expected):
                session.wait_for(rf"hart {hart} start\.\.\.", args.timeout)
            wait_for_shell(session, args.command_timeout)
            if scenario.disk_load:
                session.send("dvfs")
                session.wait_for(
                    r"Test register and update vfs domain success",
                    args.command_timeout,
                )
            session.send(f"echo {scenario.marker}")
            session.wait_for(
                rf"(?m)^{re.escape(scenario.marker)}\s*$", args.command_timeout
            )
            text = session.text()
            for failure in ("Kernel panic", "panicked at"):
                if failure in text:
                    raise RuntimeError(f"found {failure!r} in serial output")
            session.save_log(log_path, header + "result: PASS\n\n")
        except Exception:
            session.save_log(log_path, header + "result: FAIL\n\n")
            raise


def main() -> int:
    args = parse_args()
    scenarios = resolve(args.scenarios)
    if args.list:
        for name, scenario in SCENARIOS.items():
            print(f"{name:10s} {scenario.title}")
        print(f"{'all':10s} {', '.join(ALIASES['all'])}")
        return 0
    if args.scenarios == ["all"]:
        scenarios = resolve(ALIASES["all"])

    if args.log_level == "ERROR":
        print(
            "log level ERROR hides the shell-ready marker; use WARN or lower",
            file=sys.stderr,
        )
        return 2

    args.log_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    built: set[int] = set()
    failures: list[str] = []

    for scenario in scenarios:
        print(f"\n[{scenario.name}] {scenario.title}")
        try:
            if args.build and scenario.smp not in built:
                build_image(scenario.smp, args.log_level)
                built.add(scenario.smp)
            elif not image_ok(scenario.smp, args.log_level):
                if not args.build:
                    raise RuntimeError(
                        f"image {image_dir(scenario.smp)} is missing or was built "
                        f"with a different SMP/log level; rerun with --build"
                    )
            log_path = args.log_dir / f"{stamp}-alien-isolation-{scenario.name}.log"
            run_scenario(scenario, args, log_path)
            print(f"PASS {scenario.name}")
            print(f"log: {log_path}")
        except Exception as error:  # noqa: BLE001
            failures.append(scenario.name)
            print(f"FAIL {scenario.name}: {error}", file=sys.stderr)

    if failures:
        print(f"\nfailed: {', '.join(failures)}", file=sys.stderr)
        return 1
    print(f"\nPASS alien-isolation qemu: {len(scenarios)} scenarios")
    return 0


if __name__ == "__main__":
    sys.exit(main())
