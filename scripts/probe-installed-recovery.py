"""Opt-in owned recovery diagnostics; keep Firefox closed for shared registration."""
import argparse
from pathlib import Path
from qualification.browser_recovery import SCENARIOS, qualify


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--firefox", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--scenario", choices=SCENARIOS, required=True)
    args = parser.parse_args()
    qualify(args.package, args.firefox, args.report, args.scenario)


if __name__ == "__main__":
    main()
