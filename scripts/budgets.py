#!/usr/bin/env python3
"""Size and dependency budgets of the shipped binaries (docs/development.md,
guards). Standard library only.

    python3 scripts/budgets.py dependencies   # duplicated crates (tier 2)
    python3 scripts/budgets.py binaries       # sizes and crate counts (tier 3)
    python3 scripts/budgets.py binaries --update

`dependencies` counts the crates the graph holds in more than one version
and fails past the budget, or when one of the workspace's own direct
dependencies is duplicated without a declared reason: those are the
duplicates an alignment can remove.

`binaries` reads the release binaries (`cargo build --release -p
oxidgene-desktop -p oxidgene-server -p oxidgene-worker`) and the web bundle
(`just web-build`), and counts the crates each one links; it fails when one
grows past its budget plus the margin. `--update` writes the measured values
as the new budgets, after a growth that was intended — say why in the commit.
"""

import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BUDGETS = ROOT / "scripts" / "budgets.json"

# Growth tolerated before a budget fails: sizes vary with the toolchain and
# unrelated dependency patches.
SIZE_MARGIN = 0.05
CRATE_MARGIN = 3

BINARIES = {
    "desktop": ("oxidgene-desktop", None),
    "server": ("oxidgene-server", None),
    "worker": ("oxidgene-worker", None),
    "web": ("oxidgene-web", "wasm32-unknown-unknown"),
}


def cargo(*args):
    return subprocess.run(
        ["cargo", *args], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout


def duplicated_crates():
    """Crate names present in more than one version, normal and build edges,
    every target."""
    output = cargo(
        "tree", "--locked", "--workspace", "--duplicates", "--target", "all",
        "--edges", "normal,build", "--depth", "0", "--prefix", "none",
    )
    versions = {}
    for line in output.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[1].startswith("v"):
            versions.setdefault(parts[0], set()).add(parts[1])
    return {name for name, found in versions.items() if len(found) > 1}


def direct_dependencies():
    metadata = json.loads(cargo("metadata", "--locked", "--format-version", "1", "--no-deps"))
    return {
        dependency["name"]
        for package in metadata["packages"]
        for dependency in package["dependencies"]
        if dependency.get("source")
    }


def check_dependencies(budgets):
    duplicated = duplicated_crates()
    allowed = budgets["dependencies"]["direct_duplicates"]
    direct = sorted((duplicated & direct_dependencies()) - set(allowed))
    budget = budgets["dependencies"]["duplicated_crates"]
    print(f"{len(duplicated)} crates in more than one version (budget {budget})")
    failed = False
    if direct:
        print(
            "direct dependencies in two versions — align them with the framework "
            f"that pulls the other one in, or declare why in scripts/budgets.json: {direct}"
        )
        failed = True
    if len(duplicated) > budget:
        print("the graph gained duplicated crates: `cargo tree --duplicates` shows which")
        failed = True
    return failed


def crate_count(package, target):
    args = ["tree", "--locked", "-p", package, "--edges", "normal", "--prefix", "none"]
    if target:
        args += ["--target", target]
    lines = {line.split(" (")[0] for line in cargo(*args).splitlines() if line.strip()}
    return len(lines)


def size(name):
    if name == "web":
        public = ROOT / "target" / "dx" / "oxidgene-web" / "release" / "web" / "public"
        files = [p for p in public.rglob("*") if p.is_file()]
        if not files:
            raise SystemExit(f"no web bundle under {public}: run `just web-build`")
        return sum(p.stat().st_size for p in files)
    binary = ROOT / "target" / "release" / BINARIES[name][0]
    if not binary.exists():
        raise SystemExit(f"{binary} is missing: build it with `cargo build --release`")
    return binary.stat().st_size


def measure():
    return {
        name: {"bytes": size(name), "crates": crate_count(package, target)}
        for name, (package, target) in BINARIES.items()
    }


def check_binaries(budgets, update):
    measured = measure()
    report = ["| binary | bytes | budget | crates | budget |", "|---|---|---|---|---|"]
    failed = False
    for name, now in measured.items():
        budget = budgets["binaries"].get(name, {"bytes": 0, "crates": 0})
        too_big = now["bytes"] > budget["bytes"] * (1 + SIZE_MARGIN)
        too_many = now["crates"] > budget["crates"] + CRATE_MARGIN
        failed |= too_big or too_many
        report.append(
            f"| {name} | {now['bytes']:,}{' ⚠' if too_big else ''} | {budget['bytes']:,} | "
            f"{now['crates']}{' ⚠' if too_many else ''} | {budget['crates']} |"
        )
    print("\n".join(report))
    if update:
        budgets["binaries"] = measured
        BUDGETS.write_text(json.dumps(budgets, indent=2) + "\n")
        print(f"wrote {BUDGETS.relative_to(ROOT)}")
        return False
    if failed:
        print(
            f"a binary grew past its budget (+{SIZE_MARGIN:.0%} size, +{CRATE_MARGIN} crates): "
            "find what it gained; if intended, rerun with --update and say why in the commit"
        )
    return failed


def main():
    budgets = json.loads(BUDGETS.read_text())
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "dependencies":
        if "--update" in sys.argv:
            budgets["dependencies"]["duplicated_crates"] = len(duplicated_crates())
            BUDGETS.write_text(json.dumps(budgets, indent=2) + "\n")
            return 0
        return 1 if check_dependencies(budgets) else 0
    if command == "binaries":
        return 1 if check_binaries(budgets, "--update" in sys.argv) else 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main())
