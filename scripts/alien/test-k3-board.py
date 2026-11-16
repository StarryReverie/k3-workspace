#!/usr/bin/env python3
"""Repeatable K3 board scenarios built on boot-k3-board.py.

Examples:
    ./scripts/alien/test-k3-board.py --list
    ./scripts/alien/test-k3-board.py boot --build
    ./scripts/alien/test-k3-board.py smp8 --build
    ./scripts/alien/test-k3-board.py all

Board access is required. Tests are read-only by default. The PCIe
reconnaissance scenario is manual because it resets the link and needs
an installed NVMe drive.
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
ALIEN = ROOT / "alien"
OUT_K3 = ALIEN / "out-k3"
OUT_K3_DIAG = ALIEN / "out-k3-diag"
BUILD_SCRIPT = ROOT / "scripts" / "alien" / "build-k3.sh"
BOOT_SCRIPT = ROOT / "scripts" / "alien" / "boot-k3-board.py"
CPU_FILE = ALIEN / "subsystems" / "config" / "src" / "lib.rs"
MARKER_COMMAND = 'echo __TEST_"OK"__'
MARKER_PATTERN = r"__TEST_OK__"
# Known transient board-network and upload failures. Retry these only.
TRANSIENT = re.compile(
    r"Wrong Image Format|EQOS_DMA_MODE_SWR|ipaddr' not set|"
    r"TFTP error|Retry count exceeded|No link",
    re.IGNORECASE,
)


@dataclass(frozen=True)
class BuildSpec:
    label: str
    out_dir: Path
    image_name: str
    env: tuple[tuple[str, str], ...] = ()
    variant: tuple[int, bool, bool] | None = None

    @property
    def image(self) -> Path:
        return self.out_dir / self.image_name

    def key(self) -> tuple[str, tuple[tuple[str, str], ...]]:
        return (str(self.out_dir), self.env)


@dataclass(frozen=True)
class Scenario:
    name: str
    description: str
    build: BuildSpec
    marker: str
    pre_commands: tuple[str, ...] = ()
    post_commands: tuple[str, ...] = ()
    required: tuple[str, ...] = ()
    manual: bool = False


def k3_build(cpus: int = 1, *, diag: bool = False, uarttest: bool = False) -> BuildSpec:
    env: list[tuple[str, str]] = [("K3_CPUS", str(cpus))]
    label = f"k3-cpus{cpus}"
    out_dir = OUT_K3
    image_name = "alien-k3.itb"
    if diag:
        env.append(("K3_DIAG", "1"))
        label = f"k3-diag-cpus{cpus}"
        out_dir = OUT_K3_DIAG
        image_name = "alien-k3-diag.itb"
    if uarttest:
        env.append(("K3_UARTTEST", "1"))
        label += "-uarttest"
    return BuildSpec(
        label,
        out_dir,
        image_name,
        tuple(env),
        variant=(cpus, diag, uarttest),
    )


COMMON_BOOT_REQUIRED = (r"Alien:/#", MARKER_PATTERN)
BOOT_COMMANDS = (MARKER_COMMAND,)

SCENARIOS: dict[str, Scenario] = {
    "boot": Scenario(
        name="boot",
        description="1 hart, boot to shell",
        build=k3_build(1),
        marker="Alien:/#",
        post_commands=BOOT_COMMANDS,
        required=COMMON_BOOT_REQUIRED,
    ),
    "smp2": Scenario(
        name="smp2",
        description="2 harts, nproc=2",
        build=k3_build(2),
        marker="Alien:/#",
        post_commands=("/bin/busybox nproc", MARKER_COMMAND),
        required=(r"SMP: 2/2 harts online", r"(?m)^2\s*$", MARKER_PATTERN),
    ),
    "smp4": Scenario(
        name="smp4",
        description="4 harts, nproc=4",
        build=k3_build(4),
        marker="Alien:/#",
        post_commands=("/bin/busybox nproc", MARKER_COMMAND),
        required=(r"SMP: 4/4 harts online", r"(?m)^4\s*$", MARKER_PATTERN),
    ),
    "smp8": Scenario(
        name="smp8",
        description="8 harts, nproc=8",
        build=k3_build(8),
        marker="Alien:/#",
        post_commands=("/bin/busybox nproc", MARKER_COMMAND),
        required=(r"SMP: 8/8 harts online", r"(?m)^8\s*$", MARKER_PATTERN),
    ),
    "aia": Scenario(
        name="aia",
        description="interrupt console and IRQ 42",
        build=k3_build(1),
        marker="Alien:/#",
        post_commands=("/bin/busybox cat /proc/interrupts", MARKER_COMMAND),
        required=(
            r"K3 console: interrupt-driven, irq 42",
            r"(?m)^42:\s+\d+",
            MARKER_PATTERN,
        ),
    ),
    "storage": Scenario(
        name="storage",
        description="SD FAT mounted at /mnt",
        build=k3_build(1),
        marker="Alien:/#",
        post_commands=(
            "/bin/busybox ls /mnt",
            "/bin/busybox cat /mnt/HELLO.TXT",
            MARKER_COMMAND,
        ),
        required=(
            r"mount fs success at /mnt",
            r"HELLO\.TXT",
            r"alien k3 sd mount ok",
            MARKER_PATTERN,
        ),
    ),
    "uart": Scenario(
        name="uart",
        description="UART echo test",
        build=k3_build(1, diag=True, uarttest=True),
        marker="K3_UART_READY",
        post_commands=("uart-ok",),
        # tty ECHO plus the init echo produces "uuaarrtt--ookk".
        required=(r"K3_UART_READY", r"u+a+r+t+\-+o+k+"),
    ),
    "pcie-recon": Scenario(
        name="pcie-recon",
        description="read-only PCIe/NVMe U-Boot reconnaissance",
        build=k3_build(1),
        marker="Alien:/#",
        pre_commands=("pci", "pci enum", "nvme scan", "nvme info"),
        post_commands=BOOT_COMMANDS,
        required=COMMON_BOOT_REQUIRED,
        manual=True,
    ),
}

SCENARIO_ORDER = ["boot", "aia", "storage", "smp2", "smp4", "smp8", "uart", "pcie-recon"]


class CpuGuard:
    def __init__(self) -> None:
        self.original = CPU_FILE.read_text(encoding="utf-8")

    def restore(self) -> None:
        CPU_FILE.write_text(self.original, encoding="utf-8")


class BuildCache:
    def __init__(self, log_dir: Path, cpu_guard: CpuGuard) -> None:
        self.log_dir = log_dir
        self.cpu_guard = cpu_guard
        self.current: tuple[str, tuple[tuple[str, str], ...]] | None = None

    def ensure(self, build: BuildSpec) -> None:
        key = build.key()
        if self.current == key:
            return
        if not BUILD_SCRIPT.is_file():
            raise RuntimeError(f"missing build script: {BUILD_SCRIPT}")

        env = os.environ.copy()
        for name, value in build.env:
            env[name] = value
        env["OUT_DIR"] = str(build.out_dir)
        for name in ("K3_CPUS", "K3_DIAG", "K3_UARTTEST", "K3_QEMU_DISK",
                     "K3_QEMU_SHELL"):
            if name not in dict(build.env):
                env.pop(name, None)

        log_path = self.log_dir / f"build-{build.label}.log"
        log_path.parent.mkdir(parents=True, exist_ok=True)
        print(f"[build] {build.label}", flush=True)
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
            info.get("DIAG") == "1",
            info.get("UARTTEST") == "1",
        )
        return actual == build.variant


def _tail(path: Path, lines: int = 30) -> str:
    try:
        content = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return ""
    return "\n".join(content[-lines:])


def boot_command(
    scenario: Scenario,
    args: argparse.Namespace,
    log_path: Path,
) -> list[str]:
    image = scenario.build.image
    command = [
        str(BOOT_SCRIPT),
        "--board",
        args.board,
        "--image",
        str(image),
        "--marker",
        scenario.marker,
        "--log",
        str(log_path),
        "--timeout",
        str(args.timeout),
        "--boot-timeout",
        str(args.boot_timeout),
        "--post-wait",
        str(args.post_wait),
    ]
    for item in scenario.pre_commands:
        command += ["--pre-command", item]
    for item in scenario.post_commands:
        command += ["--post-command", item]
    return command


def run_scenario(scenario: Scenario, args: argparse.Namespace, log_dir: Path) -> bool:
    image = scenario.build.image
    if not image.is_file():
        print(f"missing {image}; use --build", file=sys.stderr)
        return False

    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    if args.dry_run:
        log_path = log_dir / f"{stamp}-{scenario.name}.log"
        command = boot_command(scenario, args, log_path)
        header = f"scenario: {scenario.name}\ncommand: {' '.join(command)}\n"
        print(header.strip())
        for item in scenario.pre_commands:
            print(f"  pre:  {item}")
        for item in scenario.post_commands:
            print(f"  post: {item}")
        return True

    attempts = args.retries + 1
    for attempt in range(attempts):
        suffix = "" if attempt == 0 else f".attempt{attempt + 1}"
        log_path = log_dir / f"{stamp}-{scenario.name}{suffix}.log"
        runner_log = log_dir / f"{stamp}-{scenario.name}{suffix}.runner.log"
        command = boot_command(scenario, args, log_path)
        header = f"scenario: {scenario.name}\ncommand: {' '.join(command)}\n"
        label = scenario.name if attempts == 1 else f"{scenario.name} ({attempt + 1}/{attempts})"
        print(f"[board] {label}", flush=True)

        timed_out = False
        result: subprocess.CompletedProcess[str] | None = None
        try:
            result = subprocess.run(
                command,
                cwd=ROOT,
                text=True,
                capture_output=True,
                timeout=args.timeout + 30,
            )
            runner_log.write_text(
                header
                + f"returncode: {result.returncode}\n\n"
                + result.stdout
                + "\n"
                + result.stderr,
                encoding="utf-8",
            )
        except subprocess.TimeoutExpired as error:
            timed_out = True
            runner_log.write_text(
                header + f"timeout: {error}\n",
                encoding="utf-8",
            )

        transcript = log_path.read_text(encoding="utf-8", errors="replace") if log_path.is_file() else ""
        combined = transcript
        if result is not None:
            combined += "\n" + result.stdout + "\n" + result.stderr
        cleaned = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", combined).replace("\r", "")

        if not timed_out and result is not None and result.returncode == 0:
            missing = [
                pattern
                for pattern in scenario.required
                if not re.search(pattern, cleaned, re.MULTILINE)
            ]
            if not missing:
                print(f"PASS {scenario.name}", flush=True)
                return True
            message = "missing " + ", ".join(repr(item) for item in missing)
        else:
            code = "timeout" if timed_out else str(result.returncode)
            message = f"boot script returned {code}"

        if attempt + 1 < attempts and (timed_out or TRANSIENT.search(cleaned)):
            print(f"[retry] {scenario.name}: {message}", file=sys.stderr)
            time.sleep(2)
            continue

        print(f"FAIL {scenario.name}: {message}; log: {runner_log}", file=sys.stderr)
        print("\n".join(cleaned.splitlines()[-20:]), file=sys.stderr)
        return False

    return False


def select_scenarios(args: argparse.Namespace) -> list[Scenario]:
    if args.scenario == "all":
        names = [
            name
            for name in SCENARIO_ORDER
            if args.include_manual or not SCENARIOS[name].manual
        ]
    else:
        names = [args.scenario]
    if args.filter:
        regex = re.compile(args.filter)
        names = [name for name in names if regex.search(name)]
    return [SCENARIOS[name] for name in names]


def print_list() -> None:
    for name in SCENARIO_ORDER:
        scenario = SCENARIOS[name]
        manual = " manual" if scenario.manual else ""
        print(
            f"{name:12} {scenario.description:44} "
            f"build={scenario.build.label:24} marker={scenario.marker}{manual}"
        )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "scenario",
        nargs="?",
        default="all",
        choices=[*SCENARIO_ORDER, "all"],
        help="scenario to run (default: all, excluding manual scenarios)",
    )
    parser.add_argument("--list", action="store_true", help="list scenarios")
    parser.add_argument(
        "--build",
        action=argparse.BooleanOptionalAction,
        default=None,
        help="build required images before running (default: on for all)",
    )
    parser.add_argument(
        "--include-manual",
        action="store_true",
        help="include manual scenarios in `all`",
    )
    parser.add_argument("--filter", default=None, help="scenario name filter")
    parser.add_argument(
        "--board",
        default=os.environ.get("BOARD_TYPE", "BananaPi"),
        help="board type passed to boot-k3-board.py (default: %(default)s)",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=300,
        help="overall boot script timeout (default: %(default)s)",
    )
    parser.add_argument(
        "--boot-timeout",
        type=int,
        default=45,
        help="seconds to wait for the post-boot marker (default: %(default)s)",
    )
    parser.add_argument(
        "--post-wait",
        type=float,
        default=4.0,
        help="seconds after the last post command (default: %(default)s)",
    )
    parser.add_argument(
        "--retries",
        type=int,
        default=1,
        help="retries for transient network/upload failures (default: %(default)s)",
    )
    parser.add_argument(
        "--log-dir",
        type=Path,
        default=ROOT / "logs" / "tests",
        help="directory for transcripts (default: %(default)s)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="print commands without touching the board",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.list:
        print_list()
        return 0
    if args.retries < 0:
        print("--retries must be non-negative", file=sys.stderr)
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
            if args.build and not args.dry_run:
                try:
                    cache.ensure(scenario.build)
                except Exception as error:  # noqa: BLE001
                    print(f"FAIL {scenario.name}: {error}", file=sys.stderr)
                    failures.append(scenario.name)
                    continue
            elif not args.dry_run:
                matches = cache.matches(scenario.build)
                if matches is False:
                    print(
                        f"FAIL {scenario.name}: artifact does not match "
                        f"{scenario.build.label}; use --build",
                        file=sys.stderr,
                    )
                    failures.append(scenario.name)
                    continue
                if matches is None:
                    print(
                        f"[warn] {scenario.name}: build-info missing; "
                        "cannot verify image variant",
                        file=sys.stderr,
                    )
            if not run_scenario(scenario, args, args.log_dir):
                failures.append(scenario.name)
    finally:
        cpu_guard.restore()

    if args.dry_run:
        return 0
    print()
    if failures:
        print(f"{len(failures)} scenario(s) failed: {', '.join(failures)}")
        return 1
    print(f"{len(scenarios)} scenario(s) passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
