#!/usr/bin/env python3
"""Run the Alien isolation K3 shell image on high-address QEMU.

Scenarios:
    k3-shell-1   one hart
    k3-shell-2   two harts
    k3-shell-8   eight harts
    all          all three

Each scenario waits for the user init to exec /bin/sh, checks every secondary
hart, confirms that AIA enabled the UART IRQ, performs a round trip through
the interrupt-driven console, and requires a marker line.

Examples:
    ./scripts/alien-isolation/test-k3-shell.py --build all
    ./scripts/alien-isolation/test-k3-shell.py --no-build k3-shell-2
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import time
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
ALIEN = ROOT / "alien-isolation"
BUILD = ROOT / "scripts" / "alien-isolation" / "build-k3.sh"
RUN = ROOT / "scripts" / "alien-isolation" / "run-k3-shell-qemu.sh"
sys.path.insert(0, str(ROOT / "scripts" / "alien"))

from qemu_pty import QemuSession, SessionTimeout  # noqa: E402


@dataclass(frozen=True)
class Scenario:
    name: str
    smp: int
    nvme: bool = False

    @property
    def title(self) -> str:
        extra = ", nvme" if self.nvme else ""
        return f"{self.name} (smp={self.smp}{extra}, k3 shell)"


SCENARIOS = {
    "k3-shell-1": Scenario("k3-shell-1", 1),
    "k3-shell-2": Scenario("k3-shell-2", 2),
    "k3-shell-8": Scenario("k3-shell-8", 8),
    "k3-nvme-1": Scenario("k3-nvme-1", 1, nvme=True),
    "k3-nvme-2": Scenario("k3-nvme-2", 2, nvme=True),
    "k3-nvme-8": Scenario("k3-nvme-8", 8, nvme=True),
}
ALIASES = {
    "all": [
        "k3-shell-1",
        "k3-shell-2",
        "k3-shell-8",
        "k3-nvme-1",
        "k3-nvme-2",
        "k3-nvme-8",
    ]
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "scenarios", nargs="*", default=["k3-shell-2"], help="scenario names"
    )
    parser.add_argument("--list", action="store_true", help="list scenarios")
    parser.add_argument(
        "--build",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="rebuild images before running",
    )
    parser.add_argument("--timeout", type=float, default=240.0, help="boot timeout")
    parser.add_argument(
        "--command-timeout", type=float, default=60.0, help="console timeout"
    )
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
    return ALIEN / "build" / "k3-shell" / f"smp-{smp}"


def image_ok(smp: int, nvme: bool = False) -> bool:
    out = image_dir(smp)
    if not all(
        (out / name).is_file()
        for name in ("kernel-k3", "initramfs.cpio.gz", "alien-k3-shell.itb")
    ):
        return False
    if nvme and not (out / "nvme.img").is_file():
        return False
    info = out / "build-info"
    if not info.is_file():
        return False
    values = dict(
        line.split("=", 1) for line in info.read_text().splitlines() if "=" in line
    )
    return (
        values.get("PLATFORM") == "k3"
        and values.get("MODE") == "shell"
        and values.get("SMP") == str(smp)
    )


def build_image(smp: int) -> None:
    command = [
        str(BUILD),
        "--mode",
        "shell",
        "--smp",
        str(smp),
        "--disk",
        "--out",
        str(image_dir(smp)),
    ]
    print(f"build: k3 shell smp={smp}")
    if subprocess.run(command, cwd=ROOT).returncode != 0:
        raise SystemExit(f"build failed for k3 shell smp={smp}")


def wait_for_shell(session: QemuSession, timeout: float) -> None:
    marker = "__M3_K3_READY__"
    deadline = time.monotonic() + timeout
    while True:
        session.send(f"echo {marker}")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise SessionTimeout("shell did not answer the readiness probe")
        try:
            session.wait_for(rf"(?m)^{marker}\s*$", min(3.0, remaining))
            return
        except SessionTimeout:
            time.sleep(0.5)


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
        f"mode: shell\n"
        f"smp: {scenario.smp}\n"
        f"nvme: {scenario.nvme}\n"
    )
    command = [str(RUN), "--smp", str(scenario.smp), "--out", str(out), "--timeout", "0"]
    if scenario.nvme:
        command.append("--nvme")
    with QemuSession(
        command,
        cwd=ROOT,
        env=env,
    ) as session:
        try:
            session.wait_for(r'exec args: \["sh', args.timeout)
            session.wait_for(r"AIA: enabled irq \d+", args.timeout)
            boot = session.wait_for(r"Boot hart (\d+)", args.timeout)
            boot_hart = int(boot.group(1))
            expected = set(range(scenario.smp))
            expected.discard(boot_hart)
            for hart in sorted(expected):
                session.wait_for(rf"hart {hart} start\.\.\.", args.timeout)
            wait_for_shell(session, args.command_timeout)
            session.wait_for(r"AIA/PLIC: irq \d+ delivered", args.command_timeout)
            if scenario.nvme:
                session.wait_for(r"NVMe: model=", args.timeout)
                session.wait_for(r"nvme: FAT partition lba=", args.timeout)
                session.wait_for(r"fatfs mount success", args.command_timeout)
                session.send("mapfile -t lines < /mnt/HELLO.TXT")
                session.send("echo M5_NVME:${lines[0]}")
                session.wait_for(
                    r"(?m)^M5_NVME:alien k3 nvme mount ok\s*$", args.command_timeout
                )
            session.send("echo M3_K3_OK")
            session.wait_for(r"(?m)^M3_K3_OK\s*$", args.command_timeout)
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
            print(f"{name:12s} {scenario.title}")
        print(f"{'all':12s} {', '.join(ALIASES['all'])}")
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
            elif not image_ok(scenario.smp, scenario.nvme) and not args.build:
                raise RuntimeError(
                    f"image {image_dir(scenario.smp)} is missing; rerun with --build"
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
    print(f"\nPASS alien-isolation k3 shell: {len(scenarios)} scenarios")
    return 0


if __name__ == "__main__":
    sys.exit(main())
