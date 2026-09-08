"""Compare repeated clean-target builds; no full cross-machine reproducibility claim."""
import argparse
import hashlib
import json
from pathlib import Path


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def compare(first, second, report):
    a, b = {p.name for p in first.iterdir()}, {p.name for p in second.iterdir()}
    if a != b or not a:
        raise ValueError("package leaves differ")
    hashes = {}
    for name in sorted(a):
        hashes[name] = digest(first / name)
        if hashes[name] != digest(second / name):
            raise ValueError(f"repeated clean-target build differs: {name}")
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(json.dumps({"kind": "byte-identical repeated clean Cargo target builds on this environment; not a cross-machine guarantee", "sha256": hashes}, indent=2)+"\n", encoding="utf-8")
    print("Repeated clean-target package builds are byte-identical.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("first", type=Path)
    parser.add_argument("second", type=Path)
    parser.add_argument("report", type=Path)
    args = parser.parse_args()
    compare(args.first, args.second, args.report)
