#!/usr/bin/env python3
"""Repeatable QEMU regression scenarios for Alien on k3_qemu.

Examples:
    ./scripts/alien/test-qemu.py --list
    ./scripts/alien/test-qemu.py nvme-mount --build
    ./scripts/alien/test-qemu.py all

The runner invokes the existing build and run logic, waits for serial
markers instead of fixed sleeps, saves a transcript per scenario, and
restores CPU_NUM after a multi-core build.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

from qemu_pty import QemuSession, SessionError

ROOT = Path(__file__).resolve().parent.parent.parent
ALIEN = ROOT / "alien"
OUT_DIR = ALIEN / "out-k3-qemu"
BUILD_SCRIPT = ROOT / "scripts" / "alien" / "build-k3-qemu.sh"
RUN_SCRIPT = ROOT / "scripts" / "alien" / "run-k3-qemu.sh"
CPU_FILE = ALIEN / "subsystems" / "config" / "src" / "lib.rs"
BOOT_PROMPT = r"Alien:/#"
MARKER_COMMAND = 'echo __TEST_"OK"__'
MARKER_PATTERN = r"__TEST_OK__"


@dataclass(frozen=True)
class Build:
    cpus: int = 1
    disk: bool = False
    shell: bool = True

    def key(self) -> tuple[int, bool, bool]:
        return (self.cpus, self.disk, self.shell and not self.disk)

    def label(self) -> str:
        disk = "disk" if self.disk else "nodisk"
        shell = "shell" if self.shell and not self.disk else "plain"
        return f"cpus{self.cpus}-{disk}-{shell}"


@dataclass(frozen=True)
class Qemu:
    smp: int = 1
    disk: bool = False
    nvme: bool = False
    aia: bool = False


@dataclass(frozen=True)
class Scenario:
    name: str
    description: str
    build: Build
    qemu: Qemu
    required: tuple[str, ...] = ()
    commands: tuple[tuple[str, str], ...] = (
        (MARKER_COMMAND, MARKER_PATTERN),
    )


DISK_COMMANDS = (
    ("/bin/busybox ls /tests", r"HELLO\.TXT"),
    ("/bin/busybox cat /tests/HELLO.TXT", r"alien qemu nvme mount ok"),
    (MARKER_COMMAND, MARKER_PATTERN),
)

SCENARIOS: dict[str, Scenario] = {
    "baseline": Scenario(
        name="baseline",
        description="1 hart, initramfs shell",
        build=Build(cpus=1, shell=True),
        qemu=Qemu(smp=1),
    ),
    "no-nvme": Scenario(
        name="no-nvme",
        description="1 hart, no disk, NVMe absent",
        build=Build(cpus=1, shell=True),
        qemu=Qemu(smp=1),
        required=(r"NVMe: no device",),
    ),
    "smp2": Scenario(
        name="smp2",
        description="2 harts, nproc=2",
        build=Build(cpus=2, shell=True),
        qemu=Qemu(smp=2),
        required=(r"SMP: 2/2 harts online",),
        commands=(
            ("/bin/busybox nproc", r"(?m)^2\s*$"),
            (MARKER_COMMAND, MARKER_PATTERN),
        ),
    ),
    "aia": Scenario(
        name="aia",
        description="2 harts, IMSIC/APLIC console",
        build=Build(cpus=2, shell=True),
        qemu=Qemu(smp=2, aia=True),
        required=(r"K3 console: interrupt-driven",),
    ),
    "smp8": Scenario(
        name="smp8",
        description="8 harts, nproc=8",
        build=Build(cpus=8, shell=True),
        qemu=Qemu(smp=8),
        required=(r"SMP: 8/8 harts online",),
        commands=(
            ("/bin/busybox nproc", r"(?m)^8\s*$"),
            (MARKER_COMMAND, MARKER_PATTERN),
        ),
    ),
    "virtio-disk": Scenario(
        name="virtio-disk",
        description="virtio-blk FAT mounted at /tests",
        build=Build(cpus=1, disk=True),
        qemu=Qemu(smp=1, disk=True),
        required=(r"mount fs success at /tests",),
        commands=DISK_COMMANDS,
    ),
    "nvme-mount": Scenario(
        name="nvme-mount",
        description="NVMe FAT mounted at /tests",
        build=Build(cpus=1, disk=True),
        qemu=Qemu(smp=1, disk=True, nvme=True),
        required=(r"NVMe: registered /dev/sda", r"mount fs success at /tests"),
        commands=DISK_COMMANDS,
    ),
    "nvme-aia": Scenario(
        name="nvme-aia",
        description="NVMe FAT mounted with IMSIC/APLIC console",
        build=Build(cpus=1, disk=True),
        qemu=Qemu(smp=1, disk=True, nvme=True, aia=True),
        required=(
            r"NVMe: registered /dev/sda",
            r"K3 console: interrupt-driven",
            r"mount fs success at /tests",
        ),
        commands=DISK_COMMANDS,
    ),
}

SCENARIO_ORDER = [
    "baseline",
    "no-nvme",
    "smp2",
    "aia",
    "smp8",
    "virtio-disk",
    "nvme-mount",
    "nvme-aia",
]


class CpuGuard:
    """Restore the source tree after build scripts rewrite CPU_NUM."""

    def __init__(self) -> None:
        self.original = CPU_FILE.read_text(encoding="utf-8")

    def restore(self) -> None:
        CPU_FILE.write_text(self.original, encoding="utf-8")


class BuildCache:
    def __init__(self, log_dir: Path, cpu_guard: CpuGuard) -> None:
        self.log_dir = log_dir
        self.cpu_guard = cpu_guard
        self.current: tuple[int, bool, bool] | None = None

    @staticmethod
    def info() -> dict[str, str] | None:
        path = OUT_DIR / "build-info"
        if not path.is_file():
            return None
        result: dict[str, str] = {}
        for line in path.read_text(encoding="utf-8").splitlines():
            if "=" in line:
                key, value = line.split("=", 1)
                result[key.strip()] = value.strip()
        return result

    def matches(self, build: Build) -> bool | None:
        info = self.info()
        if info is None:
            return None
        actual = (
            int(info.get("CPUS", "0")),
            info.get("DISK") == "1",
            info.get("SHELL") == "1",
        )
        return actual == build.key()

    def ensure(self, build: Build) -> None:
        key = build.key()
        if self.current == key:
            return
        if not BUILD_SCRIPT.is_file():
            raise RuntimeError(f"missing build script: {BUILD_SCRIPT}")

        env = os.environ.copy()
        env["K3_CPUS"] = str(build.cpus)
        if build.disk:
            env["K3_QEMU_DISK"] = "1"
        else:
            env.pop("K3_QEMU_DISK", None)
        if build.shell and not build.disk:
            env["K3_QEMU_SHELL"] = "1"
        else:
            env.pop("K3_QEMU_SHELL", None)

        log_path = self.log_dir / f"build-{build.label()}.log"
        log_path.parent.mkdir(parents=True, exist_ok=True)
        print(f"[build] {build.label()}", flush=True)
        with log_path.open("wb") as log:
            result = subprocess.run(
                [str(BUILD_SCRIPT)],
                cwd=ROOT,
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
            )
        self.cpu_guard.restore()
        if result.returncode != 0:
            print(f"build failed: {log_path}", file=sys.stderr)
            print(_tail(log_path), file=sys.stderr)
            raise RuntimeError(f"build failed: {build.label()}")
        self.current = key


def _tail(path: Path, lines: int = 30) -> str:
    try:
        content = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return ""
    return "\n".join(content[-lines:])


def qemu_command(config: Qemu) -> tuple[list[str], dict[str, str], str]:
    """Build the run-script invocation and a printable environment summary."""
    if not RUN_SCRIPT.is_file():
        raise RuntimeError(f"missing run script: {RUN_SCRIPT}")

    env = os.environ.copy()
    env["QEMU_SMP"] = str(config.smp)
    env["QEMU_TIMEOUT"] = "0"
    variables = [f"QEMU_SMP={config.smp}", "QEMU_TIMEOUT=0"]

    if config.aia:
        env["QEMU_AIA"] = "aplic-imsic"
        variables.append("QEMU_AIA=aplic-imsic")
    else:
        env.pop("QEMU_AIA", None)

    if config.disk:
        image = OUT_DIR / "sdcard.img"
        if not image.is_file():
            raise RuntimeError(f"missing disk image: {image}")
        env["QEMU_DISK"] = "1"
        variables.append("QEMU_DISK=1")
    else:
        env.pop("QEMU_DISK", None)

    if config.nvme:
        env["QEMU_NVME"] = "1"
        variables.append("QEMU_NVME=1")
    else:
        env.pop("QEMU_NVME", None)

    command = " ".join(variables) + f" {RUN_SCRIPT}"
    return [str(RUN_SCRIPT)], env, command


def run_scenario(scenario: Scenario, args: argparse.Namespace, log_dir: Path) -> bool:
    if not (OUT_DIR / "kernel-k3-qemu").is_file():
        print(
            f"missing {OUT_DIR / 'kernel-k3-qemu'}; use --build",
            file=sys.stderr,
        )
        return False

    try:
        argv, env, command = qemu_command(scenario.qemu)
    except Exception as error:  # noqa: BLE001
        print(f"FAIL {scenario.name}: {error}", file=sys.stderr)
        return False

    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    log_path = log_dir / f"{stamp}-{scenario.name}.log"
    header = f"scenario: {scenario.name}\ncommand: {command}\n"
    started = time.monotonic()
    session = QemuSession(
        argv,
        cwd=ROOT,
        env=env,
        verbose=args.verbose,
    )
    try:
        session.wait_for(BOOT_PROMPT, args.timeout)
        text = session.text()
        for pattern in scenario.required:
            if not re.search(pattern, text, re.MULTILINE):
                raise SessionError(f"missing required pattern: {pattern}")

        for command, expected in scenario.commands:
            session.send(command)
            session.wait_for(expected, args.command_timeout)

        session.save_log(log_path, header + "result: PASS\n\n")
        elapsed = time.monotonic() - started
        print(f"PASS {scenario.name} ({elapsed:.1f}s)", flush=True)
        return True
    except Exception as error:  # noqa: BLE001 - report test failures uniformly
        session.save_log(log_path, header + f"result: FAIL\nreason: {error}\n\n")
        elapsed = time.monotonic() - started
        print(f"FAIL {scenario.name} ({elapsed:.1f}s): {error}", file=sys.stderr)
        print(f"     log: {log_path}", file=sys.stderr)
        print(session.tail(20), file=sys.stderr)
        return False
    finally:
        session.close()


def select_scenarios(args: argparse.Namespace) -> list[Scenario]:
    if args.scenario == "all":
        names = SCENARIO_ORDER
    else:
        names = [args.scenario]
    if args.filter:
        regex = re.compile(args.filter)
        names = [name for name in names if regex.search(name)]
    return [SCENARIOS[name] for name in names]


def print_list() -> None:
    for name in SCENARIO_ORDER:
        scenario = SCENARIOS[name]
        print(
            f"{name:14} {scenario.description:42} "
            f"build={scenario.build.label()} "
            f"qemu=smp{scenario.qemu.smp}"
            f"{'-aia' if scenario.qemu.aia else ''}"
            f"{'-disk' if scenario.qemu.disk else ''}"
            f"{'-nvme' if scenario.qemu.nvme else ''}"
        )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "scenario",
        nargs="?",
        default="all",
        choices=[*SCENARIO_ORDER, "all"],
        help="scenario to run (default: all)",
    )
    parser.add_argument(
        "--list",
        action="store_true",
        help="list available scenarios and exit",
    )
    parser.add_argument(
        "--build",
        action=argparse.BooleanOptionalAction,
        default=None,
        help="build required artifacts before running (default: on for all)",
    )
    parser.add_argument(
        "--filter",
        default=None,
        help="regular expression used to select scenarios",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=90.0,
        help="seconds to wait for the shell prompt (default: %(default)s)",
    )
    parser.add_argument(
        "--command-timeout",
        type=float,
        default=10.0,
        help="seconds to wait for each command output (default: %(default)s)",
    )
    parser.add_argument(
        "--log-dir",
        type=Path,
        default=ROOT / "logs" / "tests",
        help="directory for test transcripts (default: %(default)s)",
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="print serial output while running",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.list:
        print_list()
        return 0
    if not shutil.which("qemu-system-riscv64"):
        print(
            "qemu-system-riscv64 not found; run inside `nix develop`",
            file=sys.stderr,
        )
        return 2

    scenarios = select_scenarios(args)
    if not scenarios:
        print("no scenario matched", file=sys.stderr)
        return 2

    if args.build is None:
        args.build = args.scenario == "all"

    args.log_dir.mkdir(parents=True, exist_ok=True)
    cpu_guard = CpuGuard()
    cache = BuildCache(args.log_dir, cpu_guard)
    failures: list[str] = []
    try:
        for scenario in scenarios:
            if args.build:
                try:
                    cache.ensure(scenario.build)
                except Exception as error:  # noqa: BLE001
                    print(f"FAIL {scenario.name}: {error}", file=sys.stderr)
                    failures.append(scenario.name)
                    continue
            else:
                matches = cache.matches(scenario.build)
                if matches is False:
                    print(
                        f"FAIL {scenario.name}: artifact does not match "
                        f"{scenario.build.label()}; use --build",
                        file=sys.stderr,
                    )
                    failures.append(scenario.name)
                    continue
                if matches is None:
                    print(
                        f"[warn] {scenario.name}: build-info missing; "
                        "cannot verify artifact variant",
                        file=sys.stderr,
                    )
            if not run_scenario(scenario, args, args.log_dir):
                failures.append(scenario.name)
    finally:
        cpu_guard.restore()

    print()
    if failures:
        print(f"{len(failures)} scenario(s) failed: {', '.join(failures)}")
        return 1
    print(f"{len(scenarios)} scenario(s) passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
