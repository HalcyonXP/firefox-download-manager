"""Opt-in consolidated benign loopback campaign for the actual capture candidate."""
import argparse
import os
from pathlib import Path
from qualification.candidate_run import qualify


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--firefox", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--execute-owned-browser", action="store_true")
    args = parser.parse_args()
    if not args.execute_owned_browser: parser.error("explicit owned-browser execution required")
    qualify(Path(os.path.abspath(args.package)), Path(os.path.abspath(args.candidate)), args.firefox, args.report)


if __name__ == "__main__": main()
