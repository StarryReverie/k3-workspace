#!/usr/bin/env python3
"""Run the Alien isolation K3 diagnostic kernel on high-address QEMU.

Scenarios:
    k3-1   one hart
    k3-2   two harts
    k3-8   eight harts
    all    k3-1, k3-2, k3-8

Each scenario checks that the K3-linked kernel reports memory up to
0x300000000, relocates the initramfs, prints K3_BOOT_OK, and starts every
secondary hart. The domain set is not started in this diagnostic mode.

Examples:
    ./scripts/alien-isolation/test-k3-qemu.py --build --list
    ./scripts/alien-isolation/test-k3-qemu.py --build all
    ./scripts/alien-isolation/test-k3-qemu.py --no-build k3-8
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
ALIEN = ROOT / "alien-isolation"
BUILD = ROOT / "scripts" / "alien-isolation" / "build-k3.sh"
RUN = ROOT / "scripts" / "alien-isolation" / "run-k3-qemu.sh"
sys.path.insert(0, str(ROOT / "scripts" / "alien"))

from qemu_pty import QemuSession  # noqa: E402


@dataclass(frozen=True)
class Scenario:
    name: str
    smp: int

    @property
    def title(self) -> str:
        return f"{self.name} (smp={self.smp}, k3 diagnostic)"


SCENARIOS = {
    "k3-1": Scenario("k3-1", 1),
    "k3-2": Scenario("k3-2", 2),
    "k3-8": Scenario("k3-8", 8),
}
ALIASES = {"all": ["k3-1", "k3-2", "k3-8"]}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "scenarios", nargs="*", default=["k3-1"], help="scenario names"
    )
    parser.add_argument("--list", action="store_true", help="list scenarios")
    parser.add_argument(
        "--build",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="rebuild images before running (default: %(default)s)",
    )
    parser.add_argument("--timeout", type=float, default=120.0, help="boot timeout")
    parser.add_argument(
        "--log-dir", type=Path, default=ROOT / "logs" / "tests"
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
    return ALIEN / "build" / "k3" / f"smp-{smp}"


def image_ok(smp: int) -> bool:
    out = image_dir(smp)
    if not all(
        (out / name).is_file()
        for name in ("kernel-k3", "initramfs.cpio.gz", "alien-k3-diag.itb")
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
    return values.get("PLATFORM") == "k3" and values.get("SMP") == str(smp)


def build_image(smp: int) -> None:
    command = [str(BUILD), "--smp", str(smp), "--out", str(image_dir(smp))]
    print(f"build: k3 smp={smp}")
    if subprocess.run(command, cwd=ROOT).returncode != 0:
        raise SystemExit(f"build failed for k3 smp={smp}")


def run_scenario(
    scenario: Scenario, args: argparse.Namespace, log_path: Path
) -> None:
    out = image_dir(scenario.smp)
    env = os.environ.copy()
    env.update(
        {
            "QEMU_TIMEOUT": "0",
            "OUT_DIR": str(out),
            "QEMU_SMP": str(scenario.smp),
        }
    )
    header = (
        f"scenario: {scenario.name}\n"
        f"platform: k3\n"
        f"smp: {scenario.smp}\n"
        f"mode: diagnostic\n"
    )
    with QemuSession(
        [str(RUN), "--smp", str(scenario.smp), "--out", str(out), "--timeout", "0"],
        cwd=ROOT,
        env=env,
    ) as session:
        try:
            session.wait_for(r"K3_BOOT_OK", args.timeout)
            session.wait_for(r"Memory: 0x[0-9a-f]+\.\.0x300000000", args.timeout)
            session.wait_for(r"Relocate initrd data to 0x[0-9a-f]+", args.timeout)
            boot = session.wait_for(r"Boot hart (\d+)", args.timeout)
            boot_hart = int(boot.group(1))
            expected = set(range(scenario.smp))
            expected.discard(boot_hart)
            for hart in sorted(expected):
                session.wait_for(rf"hart {hart} start\.\.\.", args.timeout)
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
            print(f"{name:8s} {scenario.title}")
        print(f"{'all':8s} {', '.join(ALIASES['all'])}")
        return 0

    args.log_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    built: set[int] = set()
    failures: list[str] = []

    for scenario in scenarios:
        print(f"\n[{scenario.name}] {scenario.title}")
        try:
            if args.build and scenario.smp not in built:
                build_image(scenario.smp)
                built.add(scenario.smp)
            elif not image_ok(scenario.smp) and not args.build:
                raise RuntimeError(
                    f"image {image_dir(scenario.smp)} is missing; "
                    f"rerun with --build"
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
    print(f"\nPASS alien-isolation k3 qemu: {len(scenarios)} scenarios")
    return 0


if __name__ == "__main__":
    sys.exit(main())
