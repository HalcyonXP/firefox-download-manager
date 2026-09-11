"""Observe only the separately built fileless privileged protection-service probe."""
import argparse
from pathlib import Path
from qualification.protection_run import run


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--probe',type=Path,required=True)
    parser.add_argument('--firefox',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--execute-owned-browser',action='store_true')
    args = parser.parse_args()
    if not args.execute_owned_browser: parser.error('explicit owned browser execution required')
    run(args.probe,args.firefox,args.report)


if __name__=='__main__': main()
