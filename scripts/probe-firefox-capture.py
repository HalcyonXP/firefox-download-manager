"""Owned loopback Firefox API probe; temporary diagnostic add-on, NOT product acceptance."""
import argparse
from pathlib import Path

from qualification.capture import run


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--firefox", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--execute-owned-browser", action="store_true", required=True,
                        help="launch an isolated visible browser with existing closed-app preflights")
    args = parser.parse_args()
    try:
        run(args.firefox, args.report)
    except Exception:
        raise SystemExit("Capture API probe refused or failed; private owned domain preserved; no success report.") from None
