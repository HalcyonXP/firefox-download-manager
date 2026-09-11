"""Build only the separately identified fileless Firefox protection-service probe."""
import argparse
from pathlib import Path
from qualification.protection_input import build


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    identity = build(args.output)
    print('Built fileless protection probe; qualification:false; XPI SHA256 '+identity['xpi_sha256'])


if __name__ == '__main__': main()
