"""Owned normal unsigned-XPI UI observation, not installed handoff acceptance."""
import argparse
from pathlib import Path
from qualification.xpi_persistence import run


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--firefox", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--execute-owned-browser", action="store_true", required=True)
    args = parser.parse_args()
    try:
        outcome = run(args.package, args.firefox, args.report)
    except Exception:
        raise SystemExit("XPI observation refused or failed; preserve owned domain; no acceptance claimed.") from None
    if outcome != "installed":
        raise SystemExit("Signature requirement observed in owned defaults; diagnostic report is not persistent-install acceptance.")
