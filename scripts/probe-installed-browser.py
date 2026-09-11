"""Opt-in owned installed/browser diagnostic; never normal-profile automation."""
import argparse
from pathlib import Path
from qualification.browser_installed import qualify
from qualification.installed import FAULTS


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--firefox", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--fault", choices=FAULTS)
    parser.add_argument("--scenario", choices=("nominal", "missing-terminal", "cross-origin"), default="nominal")
    args = parser.parse_args()
    qualify(args.package, args.firefox, args.report, args.fault, args.scenario)


if __name__ == "__main__":
    main()
