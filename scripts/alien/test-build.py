#!/usr/bin/env python3
"""Build and artifact checks for Alien QEMU and K3 images.

Examples:
    ./scripts/alien/test-build.py --list
    ./scripts/alien/test-build.py k3
    ./scripts/alien/test-build.py all

The runner invokes the existing build scripts, checks expected files,
verifies ELF entry addresses and validates FIT images with mkimage.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent.parent
ALIEN = ROOT / "alien"
OUT_QEMU = ALIEN / "out-qemu"
OUT_K3_QEMU = ALIEN / "out-k3-qemu"
OUT_K3 = ALIEN / "out-k3"
OUT_K3_DIAG = ALIEN / "out-k3-diag"
BUILD_QEMU = ROOT / "scripts" / "alien" / "build-qemu.sh"
BUILD_K3_QEMU = ROOT / "scripts" / "alien" / "build-k3-qemu.sh"
BUILD_K3 = ROOT / "scripts" / "alien" / "build-k3.sh"
CPU_FILE = ALIEN / "subsystems" / "config" / "src" / "lib.rs"


@dataclass(frozen=True)
class BuildScenario:
    name: str
    description: str
    script: Path
    out_dir: Path
    env: tuple[tuple[str, str], ...]
    artifacts: tuple[Path, ...]
    entry_points: tuple[tuple[Path, int], ...] = ()
    fit_images: tuple[Path, ...] = ()
    smp: tuple[Path, int] | None = None


SCENARIOS: dict[str, BuildScenario] = {
    "qemu": BuildScenario(
        name="qemu",
        description="upstream qemu baseline",
        script=BUILD_QEMU,
        out_dir=OUT_QEMU,
        env=(),
        artifacts=(
            OUT_QEMU / "kernel-qemu",
            OUT_QEMU / "sdcard.img",
            OUT_QEMU / "initramfs.cpio.gz",
        ),
        entry_points=((OUT_QEMU / "kernel-qemu", 0x80200000),),
    ),
    "k3-qemu": BuildScenario(
        name="k3-qemu",
        description="k3_qemu disk regression image",
        script=BUILD_K3_QEMU,
        out_dir=OUT_K3_QEMU,
        env=(("K3_QEMU_DISK", "1"),),
        artifacts=(
            OUT_K3_QEMU / "kernel-k3-qemu",
            OUT_K3_QEMU / "sdcard.img",
            OUT_K3_QEMU / "build-info",
        ),
        entry_points=((OUT_K3_QEMU / "kernel-k3-qemu", 0x80200000),),
    ),
    "k3": BuildScenario(
        name="k3",
        description="K3 single-core FIT image",
        script=BUILD_K3,
        out_dir=OUT_K3,
        env=(("K3_CPUS", "1"),),
        artifacts=(
            OUT_K3 / "kernel-k3",
            OUT_K3 / "alien-k3.itb",
            OUT_K3 / "build-info",
            ALIEN / "alien-k3.bin",
            ALIEN / "alien-k3.bin.gz",
        ),
        entry_points=((OUT_K3 / "kernel-k3", 0x102200000),),
        fit_images=(OUT_K3 / "alien-k3.itb",),
        smp=(OUT_K3 / "smp", 1),
    ),
    "k3-8core": BuildScenario(
        name="k3-8core",
        description="K3 eight-core FIT image",
        script=BUILD_K3,
        out_dir=OUT_K3,
        env=(("K3_CPUS", "8"),),
        artifacts=(
            OUT_K3 / "kernel-k3",
            OUT_K3 / "alien-k3.itb",
            OUT_K3 / "build-info",
        ),
        entry_points=((OUT_K3 / "kernel-k3", 0x102200000),),
        fit_images=(OUT_K3 / "alien-k3.itb",),
        smp=(OUT_K3 / "smp", 8),
    ),
    "k3-diag-uart": BuildScenario(
        name="k3-diag-uart",
        description="K3 diagnostic image with UART self-test",
        script=BUILD_K3,
        out_dir=OUT_K3_DIAG,
        env=(("K3_DIAG", "1"), ("K3_UARTTEST", "1"), ("K3_CPUS", "1")),
        artifacts=(
            OUT_K3_DIAG / "kernel-k3",
            OUT_K3_DIAG / "alien-k3-diag.itb",
            OUT_K3_DIAG / "build-info",
        ),
        entry_points=((OUT_K3_DIAG / "kernel-k3", 0x102200000),),
        fit_images=(OUT_K3_DIAG / "alien-k3-diag.itb",),
        smp=(OUT_K3_DIAG / "smp", 1),
    ),
}

SCENARIO_ORDER = ["qemu", "k3-qemu", "k3", "k3-8core", "k3-diag-uart"]


class CpuGuard:
    def __init__(self) -> None:
        self.original = CPU_FILE.read_text(encoding="utf-8")

    def restore(self) -> None:
        CPU_FILE.write_text(self.original, encoding="utf-8")


def tail(path: Path, lines: int = 30) -> str:
    try:
        content = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return ""
    return "\n".join(content[-lines:])


def run_build(scenario: BuildScenario, log_dir: Path, cpu_guard: CpuGuard) -> Path:
    env = os.environ.copy()
    for name, value in scenario.env:
        env[name] = value
    env["OUT_DIR"] = str(scenario.out_dir)
    for name in ("K3_CPUS", "K3_DIAG", "K3_UARTTEST", "K3_QEMU_DISK",
                 "K3_QEMU_SHELL"):
        if name not in dict(scenario.env):
            env.pop(name, None)

    log_path = log_dir / f"buildcheck-{scenario.name}.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    print(f"[build] {scenario.name}", flush=True)
    with log_path.open("wb") as log:
        result = subprocess.run(
            [str(scenario.script)],
            cwd=ROOT,
            env=env,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
    cpu_guard.restore()
    if result.returncode != 0:
        print(f"FAIL {scenario.name}: build failed; log: {log_path}", file=sys.stderr)
        print(tail(log_path), file=sys.stderr)
        raise RuntimeError(f"build failed: {scenario.name}")
    return log_path


def elf_entry(path: Path) -> int:
    readelf = shutil.which("readelf") or shutil.which("llvm-readelf")
    if not readelf:
        raise RuntimeError("readelf or llvm-readelf not found")
    result = subprocess.run(
        [readelf, "-h", str(path)],
        text=True,
        capture_output=True,
        check=True,
    )
    match = re.search(r"Entry point address:\s+(0x[0-9a-fA-F]+)", result.stdout)
    if not match:
        raise RuntimeError(f"cannot parse entry point: {path}")
    return int(match.group(1), 16)


def check_scenario(
    scenario: BuildScenario, log_dir: Path, cpu_guard: CpuGuard
) -> bool:
    try:
        log_path = run_build(scenario, log_dir, cpu_guard)
    except Exception as error:  # noqa: BLE001
        print(f"FAIL {scenario.name}: {error}", file=sys.stderr)
        return False

    try:
        missing = [path for path in scenario.artifacts if not path.is_file()]
        if missing:
            raise RuntimeError(
                "missing artifacts: " + ", ".join(str(path) for path in missing)
            )
        for path, expected in scenario.entry_points:
            actual = elf_entry(path)
            if actual != expected:
                raise RuntimeError(
                    f"{path} entry {actual:#x}, expected {expected:#x}"
                )
        mkimage = shutil.which("mkimage")
        if scenario.fit_images and not mkimage:
            raise RuntimeError("mkimage not found")
        for image in scenario.fit_images:
            subprocess.run(
                [mkimage, "-l", str(image)],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
                check=True,
            )
        if scenario.smp is not None:
            path, expected = scenario.smp
            actual = int(path.read_text(encoding="utf-8").strip())
            if actual != expected:
                raise RuntimeError(f"{path} contains {actual}, expected {expected}")
    except Exception as error:  # noqa: BLE001
        print(f"FAIL {scenario.name}: {error}", file=sys.stderr)
        print(f"     build log: {log_path}", file=sys.stderr)
        return False

    print(f"PASS {scenario.name}", flush=True)
    return True


def select_scenarios(args: argparse.Namespace) -> list[BuildScenario]:
    names = SCENARIO_ORDER if args.scenario == "all" else [args.scenario]
    if args.filter:
        regex = re.compile(args.filter)
        names = [name for name in names if regex.search(name)]
    return [SCENARIOS[name] for name in names]


def print_list() -> None:
    for name in SCENARIO_ORDER:
        scenario = SCENARIOS[name]
        print(
            f"{name:14} {scenario.description:44} "
            f"build={scenario.script.name}"
        )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "scenario",
        nargs="?",
        default="all",
        choices=[*SCENARIO_ORDER, "all"],
        help="build scenario (default: all)",
    )
    parser.add_argument("--list", action="store_true", help="list scenarios")
    parser.add_argument("--filter", default=None, help="scenario name filter")
    parser.add_argument(
        "--log-dir",
        type=Path,
        default=ROOT / "logs" / "tests",
        help="directory for build logs (default: %(default)s)",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.list:
        print_list()
        return 0

    scenarios = select_scenarios(args)
    if not scenarios:
        print("no scenario matched", file=sys.stderr)
        return 2

    args.log_dir.mkdir(parents=True, exist_ok=True)
    cpu_guard = CpuGuard()
    failures: list[str] = []
    try:
        for scenario in scenarios:
            if not check_scenario(scenario, args.log_dir, cpu_guard):
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
