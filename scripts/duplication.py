# /// script
# requires-python = ">=3.10"
# dependencies = ["lizard==1.24.0"]
# ///
"""Fail when the share of duplicated Rust code grows past its budget.

Lizard's duplicate extension (`-Eduplicate`) finds repeated token sequences
across `crates/` and `apps/` and reports a total duplicate rate. The rate
measured when the budget was last set, plus a small margin, is the budget:
code copied instead of shared pushes it over, and the fix is to factor the
copy (docs/development.md, guards). Generated or tabular code is left out:
the migration, the translation tables and the `mod.rs` lists.

`uv run scripts/duplication.py` checks; `--report` prints every duplicate
block as well. Lower `BUDGET` when a refactoring brings the rate down.
"""

import re
import subprocess
import sys

# Percent of duplicated code: the rate measured on 2026-10-02, after both fix
# waves (2.97 %), plus a margin for unrelated churn.
BUDGET = 3.2

EXCLUDES = ["*/target/*", "*/migration/*", "*/i18n/*", "*/mod.rs", "*/fuzz/*"]


def main() -> int:
    command = [sys.executable, "-m", "lizard", "-Eduplicate", "-l", "rust", "crates", "apps"]
    for pattern in EXCLUDES:
        command += ["-x", pattern]
    output = subprocess.run(command, capture_output=True, text=True, check=False).stdout
    match = re.search(r"Total duplicate rate: ([0-9.]+)%", output)
    if not match:
        print(output[-2000:])
        print("lizard reported no duplicate rate", file=sys.stderr)
        return 2
    if "--report" in sys.argv:
        start = output.find("Duplicates")
        print(output[start:] if start >= 0 else output)
    rate = float(match.group(1))
    print(f"duplicate rate {rate:.2f} % (budget {BUDGET:.2f} %)")
    if rate > BUDGET:
        print(
            "duplicated code grew past its budget: run `uv run scripts/duplication.py --report` "
            "and factor the new copies",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
