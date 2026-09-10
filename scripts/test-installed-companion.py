"""Opt-in real current-user registration slice; no Firefox or persistent XPI test.

Requires freshly closed Firefox/helpers and absent host registration. Uses only a
new isolated installation/profile/Programs domain. Never closes unowned apps.
"""
import argparse
from pathlib import Path

from qualification.installed import FAULTS, qualify


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--execute-owned-install", action="store_true", required=True,
                        help="explicitly select the real registration-mutating fixture")
    parser.add_argument("--fail-at", choices=FAULTS, help="inject failure; never authorizes success")
    args = parser.parse_args()
    try:
        qualify(args.package, args.report, args.fail_at)
    except Exception:
        raise SystemExit("Installed slice refused or failed; preserve owned state; no success authorized.") from None
