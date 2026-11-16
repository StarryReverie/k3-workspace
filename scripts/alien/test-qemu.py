#!/usr/bin/env python3
"""Repeatable QEMU regression scenarios for Alien.

Examples:
    ./scripts/alien/test-qemu.py --list
    ./scripts/alien/test-qemu.py nvme-mount --build
    ./scripts/alien/test-qemu.py all

The runner invokes the existing build and run scripts, waits for serial
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
OUT_K3_QEMU = ALIEN / "out-k3-qemu"
OUT_K3_DIAG = ALIEN / "out-k3-diag"
OUT_QEMU = ALIEN / "out-qemu"
BUILD_K3_QEMU = ROOT / "scripts" / "alien" / "build-k3-qemu.sh"
BUILD_K3 = ROOT / "scripts" / "alien" / "build-k3.sh"
BUILD_QEMU = ROOT / "scripts" / "alien" / "build-qemu.sh"
RUN_K3_QEMU = ROOT / "scripts" / "alien" / "run-k3-qemu.sh"
RUN_K3_HIGH = ROOT / "scripts" / "alien" / "run-k3-high-qemu.sh"
RUN_QEMU = ROOT / "scripts" / "alien" / "run-qemu.sh"
CPU_FILE = ALIEN / "subsystems" / "config" / "src" / "lib.rs"
BOOT_PROMPT = r"Alien:/#"
MARKER_COMMAND = 'echo __TEST_"OK"__'
MARKER_PATTERN = r"__TEST_OK__"
WRITE_COMMAND = 'printf \'WRITE_%s\\n\' OK > /tests/WRITE.TXT && echo __WRITE_"DONE"__'
WRITE_DONE = r"__WRITE_DONE__"
WRITE_CONTENT = r"WRITE_OK"
IRQ_PATTERN = r"(?m)^10:\s+\d+"


@dataclass(frozen=True)
class BuildSpec:
    label: str
    script: Path
    out_dir: Path
    env: tuple[tuple[str, str], ...] = ()
    # (cpus, disk, shell) for k3_qemu build-info verification.
    variant: tuple[int, bool, bool] | None = None

    def key(self) -> tuple[str, str, tuple[tuple[str, str], ...]]:
        return (str(self.script), str(self.out_dir), self.env)

    def env_dict(self) -> dict[str, str]:
        return dict(self.env)


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
    build: BuildSpec
    runner: str
    qemu: Qemu | None = None
    boot: str = BOOT_PROMPT
    required: tuple[str, ...] = ()
    commands: tuple[tuple[str, str], ...] = (
        (MARKER_COMMAND, MARKER_PATTERN),
    )


def k3_qemu_build(
    cpus: int, *, disk: bool = False, shell: bool = True
) -> BuildSpec:
    env: list[tuple[str, str]] = [("K3_CPUS", str(cpus))]
    if disk:
        env.append(("K3_QEMU_DISK", "1"))
    elif shell:
        env.append(("K3_QEMU_SHELL", "1"))
    disk_part = "disk" if disk else "nodisk"
    shell_part = "shell" if shell and not disk else "plain"
    return BuildSpec(
        label=f"k3q-cpus{cpus}-{disk_part}-{shell_part}",
        script=BUILD_K3_QEMU,
        out_dir=OUT_K3_QEMU,
        env=tuple(env),
        variant=(cpus, disk, shell and not disk),
    )


BUILD_QEMU_BASELINE = BuildSpec(
    label="qemu-baseline",
    script=BUILD_QEMU,
    out_dir=OUT_QEMU,
)

BUILD_K3_DIAG = BuildSpec(
    label="k3-diag",
    script=BUILD_K3,
    out_dir=OUT_K3_DIAG,
    env=(("K3_DIAG", "1"), ("K3_CPUS", "1")),
)

READ_DISK_COMMANDS = (
    ("/bin/busybox ls /tests", r"HELLO\.TXT"),
    ("/bin/busybox cat /tests/HELLO.TXT", r"alien qemu nvme mount ok"),
    (MARKER_COMMAND, MARKER_PATTERN),
)

WRITE_DISK_COMMANDS = (
    ("/bin/busybox ls /tests", r"HELLO\.TXT"),
    ("/bin/busybox cat /tests/HELLO.TXT", r"alien qemu nvme mount ok"),
    (WRITE_COMMAND, WRITE_DONE),
    ("/bin/busybox cat /tests/WRITE.TXT", WRITE_CONTENT),
    (MARKER_COMMAND, MARKER_PATTERN),
)

AIA_COMMANDS = (
    ("/bin/busybox cat /proc/interrupts", IRQ_PATTERN),
    (MARKER_COMMAND, MARKER_PATTERN),
)

SCENARIOS: dict[str, Scenario] = {
    "qemu-baseline": Scenario(
        name="qemu-baseline",
        description="upstream qemu feature, virtio disk",
        build=BUILD_QEMU_BASELINE,
        runner="qemu",
        required=(r"mount fs success",),
        commands=(
            ("/bin/busybox ls /tests", r"init"),
            (MARKER_COMMAND, MARKER_PATTERN),
        ),
    ),
    "high": Scenario(
        name="high",
        description="real K3 ELF at 0x102200000, high memory and page table",
        build=BUILD_K3_DIAG,
        runner="high",
        # User output goes to the K3 UART address, which is backed by QEMU
        # RAM in this layout. Assert kernel-level high-address progress.
        boot=r"kthread_init start\.\.\.",
        required=(
            r"Memory: 0x102000000\.\.0x300000000",
            r"Frame allocator init success",
            r"Build kernel address space success",
        ),
        commands=(),
    ),
    "baseline": Scenario(
        name="baseline",
        description="1 hart, initramfs shell",
        build=k3_qemu_build(1, shell=True),
        runner="k3_qemu",
        qemu=Qemu(smp=1),
    ),
    "no-nvme": Scenario(
        name="no-nvme",
        description="1 hart, no disk, NVMe absent",
        build=k3_qemu_build(1, shell=True),
        runner="k3_qemu",
        qemu=Qemu(smp=1),
        required=(r"NVMe: no device",),
    ),
    "aia1": Scenario(
        name="aia1",
        description="1 hart, IMSIC/APLIC console",
        build=k3_qemu_build(1, shell=True),
        runner="k3_qemu",
        qemu=Qemu(smp=1, aia=True),
        required=(r"K3 console: interrupt-driven",),
        commands=AIA_COMMANDS,
    ),
    "smp2": Scenario(
        name="smp2",
        description="2 harts, nproc=2",
        build=k3_qemu_build(2, shell=True),
        runner="k3_qemu",
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
        build=k3_qemu_build(2, shell=True),
        runner="k3_qemu",
        qemu=Qemu(smp=2, aia=True),
        required=(r"K3 console: interrupt-driven",),
        commands=AIA_COMMANDS,
    ),
    "smp4": Scenario(
        name="smp4",
        description="4 harts, nproc=4",
        build=k3_qemu_build(4, shell=True),
        runner="k3_qemu",
        qemu=Qemu(smp=4),
        required=(r"SMP: 4/4 harts online",),
        commands=(
            ("/bin/busybox nproc", r"(?m)^4\s*$"),
            (MARKER_COMMAND, MARKER_PATTERN),
        ),
    ),
    "aia4": Scenario(
        name="aia4",
        description="4 harts, IMSIC/APLIC console",
        build=k3_qemu_build(4, shell=True),
        runner="k3_qemu",
        qemu=Qemu(smp=4, aia=True),
        required=(r"SMP: 4/4 harts online", r"K3 console: interrupt-driven"),
        commands=AIA_COMMANDS,
    ),
    "smp8": Scenario(
        name="smp8",
        description="8 harts, nproc=8",
        build=k3_qemu_build(8, shell=True),
        runner="k3_qemu",
        qemu=Qemu(smp=8),
        required=(r"SMP: 8/8 harts online",),
        commands=(
            ("/bin/busybox nproc", r"(?m)^8\s*$"),
            (MARKER_COMMAND, MARKER_PATTERN),
        ),
    ),
    "aia8": Scenario(
        name="aia8",
        description="8 harts, IMSIC/APLIC console",
        build=k3_qemu_build(8, shell=True),
        runner="k3_qemu",
        qemu=Qemu(smp=8, aia=True),
        required=(r"SMP: 8/8 harts online", r"K3 console: interrupt-driven"),
        commands=AIA_COMMANDS,
    ),
    "virtio-disk": Scenario(
        name="virtio-disk",
        description="virtio-blk FAT read and write",
        build=k3_qemu_build(1, disk=True),
        runner="k3_qemu",
        qemu=Qemu(smp=1, disk=True),
        required=(r"mount fs success at /tests",),
        commands=WRITE_DISK_COMMANDS,
    ),
    "virtio-aia": Scenario(
        name="virtio-aia",
        description="virtio-blk FAT with IMSIC/APLIC console",
        build=k3_qemu_build(1, disk=True),
        runner="k3_qemu",
        qemu=Qemu(smp=1, disk=True, aia=True),
        required=(r"mount fs success at /tests", r"K3 console: interrupt-driven"),
        commands=WRITE_DISK_COMMANDS,
    ),
    "nvme-mount": Scenario(
        name="nvme-mount",
        description="NVMe FAT mounted at /tests",
        build=k3_qemu_build(1, disk=True),
        runner="k3_qemu",
        qemu=Qemu(smp=1, disk=True, nvme=True),
        required=(r"NVMe: registered /dev/sda", r"mount fs success at /tests"),
        commands=READ_DISK_COMMANDS,
    ),
    "nvme-aia": Scenario(
        name="nvme-aia",
        description="NVMe FAT mounted with IMSIC/APLIC console",
        build=k3_qemu_build(1, disk=True),
        runner="k3_qemu",
        qemu=Qemu(smp=1, disk=True, nvme=True, aia=True),
        required=(
            r"NVMe: registered /dev/sda",
            r"K3 console: interrupt-driven",
            r"mount fs success at /tests",
        ),
        commands=READ_DISK_COMMANDS,
    ),
}

SCENARIO_ORDER = [
    "qemu-baseline",
    "high",
    "baseline",
    "no-nvme",
    "aia1",
    "smp2",
    "aia",
    "smp4",
    "aia4",
    "smp8",
    "aia8",
    "virtio-disk",
    "virtio-aia",
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
        self.current: tuple[str, str, tuple[tuple[str, str], ...]] | None = None

    def ensure(self, build: BuildSpec) -> None:
        key = build.key()
        if self.current == key:
            return
        if not build.script.is_file():
            raise RuntimeError(f"missing build script: {build.script}")

        env = os.environ.copy()
        for name, value in build.env:
            env[name] = value
        env["OUT_DIR"] = str(build.out_dir)
        for name in ("K3_CPUS", "K3_QEMU_DISK", "K3_QEMU_SHELL", "K3_DIAG",
                     "K3_UARTTEST"):
            if name not in build.env_dict():
                env.pop(name, None)

        log_path = self.log_dir / f"build-{build.label}.log"
        log_path.parent.mkdir(parents=True, exist_ok=True)
        print(f"[build] {build.label}", flush=True)
        with log_path.open("wb") as log:
            result = subprocess.run(
                [str(build.script)],
                cwd=ROOT,
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
            )
        self.cpu_guard.restore()
        if result.returncode != 0:
            print(f"build failed: {log_path}", file=sys.stderr)
            print(_tail(log_path), file=sys.stderr)
            raise RuntimeError(f"build failed: {build.label}")
        self.current = key

    @staticmethod
    def info(out_dir: Path) -> dict[str, str] | None:
        path = out_dir / "build-info"
        if not path.is_file():
            return None
        result: dict[str, str] = {}
        for line in path.read_text(encoding="utf-8").splitlines():
            if "=" in line:
                key, value = line.split("=", 1)
                result[key.strip()] = value.strip()
        return result

    def matches(self, build: BuildSpec) -> bool | None:
        if build.variant is None:
            return None
        info = self.info(build.out_dir)
        if info is None:
            return None
        actual = (
            int(info.get("CPUS", "0")),
            info.get("DISK") == "1",
            info.get("SHELL") == "1",
        )
        return actual == build.variant


def _tail(path: Path, lines: int = 30) -> str:
    try:
        content = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return ""
    return "\n".join(content[-lines:])


def required_artifacts(runner: str) -> tuple[Path, ...]:
    if runner == "k3_qemu":
        return (OUT_K3_QEMU / "kernel-k3-qemu",)
    if runner == "high":
        return (OUT_K3_DIAG / "kernel-k3",)
    if runner == "qemu":
        return (
            OUT_QEMU / "kernel-qemu",
            OUT_QEMU / "sdcard.img",
            OUT_QEMU / "initramfs.cpio.gz",
        )
    raise RuntimeError(f"unknown runner: {runner}")


def run_command(scenario: Scenario) -> tuple[list[str], dict[str, str], str]:
    env = os.environ.copy()
    if scenario.runner == "k3_qemu":
        qemu = scenario.qemu  # type: ignore[attr-defined]
        env["QEMU_SMP"] = str(qemu.smp)
        env["QEMU_TIMEOUT"] = "0"
        env["OUT_DIR"] = str(OUT_K3_QEMU)
        variables = [f"QEMU_SMP={qemu.smp}", "QEMU_TIMEOUT=0"]
        if qemu.aia:
            env["QEMU_AIA"] = "aplic-imsic"
            variables.append("QEMU_AIA=aplic-imsic")
        else:
            env.pop("QEMU_AIA", None)
        if qemu.disk:
            image = OUT_K3_QEMU / "sdcard.img"
            if not image.is_file():
                raise RuntimeError(f"missing disk image: {image}")
            env["QEMU_DISK"] = "1"
            variables.append("QEMU_DISK=1")
        else:
            env.pop("QEMU_DISK", None)
        if qemu.nvme:
            env["QEMU_NVME"] = "1"
            variables.append("QEMU_NVME=1")
        else:
            env.pop("QEMU_NVME", None)
        script = RUN_K3_QEMU
    elif scenario.runner == "high":
        env["QEMU_MEM"] = "10G"
        env["QEMU_TIMEOUT"] = "0"
        env["OUT_DIR"] = str(OUT_K3_DIAG)
        variables = ["QEMU_MEM=10G", "QEMU_TIMEOUT=0"]
        script = RUN_K3_HIGH
    elif scenario.runner == "qemu":
        env["QEMU_TIMEOUT"] = "0"
        env["OUT_DIR"] = str(OUT_QEMU)
        variables = ["QEMU_TIMEOUT=0"]
        script = RUN_QEMU
    else:
        raise RuntimeError(f"unknown runner: {scenario.runner}")

    return [str(script)], env, " ".join(variables) + f" {script}"


def run_scenario(scenario: Scenario, args: argparse.Namespace, log_dir: Path) -> bool:
    missing = [path for path in required_artifacts(scenario.runner) if not path.is_file()]
    if missing:
        print(
            f"missing {', '.join(str(path) for path in missing)}; use --build",
            file=sys.stderr,
        )
        return False

    try:
        argv, env, command = run_command(scenario)
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
        session.wait_for(scenario.boot, args.timeout)
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
        runner = scenario.runner
        detail = runner
        if runner == "k3_qemu":
            qemu = scenario.qemu  # type: ignore[attr-defined]
            detail = (
                f"smp{qemu.smp}"
                f"{'-aia' if qemu.aia else ''}"
                f"{'-disk' if qemu.disk else ''}"
                f"{'-nvme' if qemu.nvme else ''}"
            )
        print(
            f"{name:14} {scenario.description:42} "
            f"build={scenario.build.label:24} run={detail}"
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
        help="seconds to wait for the boot marker (default: %(default)s)",
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
                        f"{scenario.build.label}; use --build",
                        file=sys.stderr,
                    )
                    failures.append(scenario.name)
                    continue
                if matches is None and scenario.build.variant is not None:
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
