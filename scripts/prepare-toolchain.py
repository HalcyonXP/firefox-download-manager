"""Fetch and verify the reviewed build-only LLVM/MinGW archive; no system installation."""
from pathlib import Path, PurePosixPath
import hashlib
import stat
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
NAME = "llvm-mingw-20260826-ucrt-x86_64"
ARCHIVE_SHA256 = "ae601f4e0f72bbdf441ad2df8bb16f037e2e9251559ea6b37b4057aef39c06c3"
ARCHIVE_SIZE = 190721391
URL = f"https://github.com/mstorsjo/llvm-mingw/releases/download/20260826/{NAME}.zip"


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def directory(path):
    path.mkdir(parents=True, exist_ok=True)
    for entry in [path, *path.parents]:
        if not entry.is_dir() or entry.lstat().st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT:
            raise ValueError("toolchain directory is not ordinary")


def prepare():
    root = ROOT / "target" / "toolchains"
    directory(root)
    archive = root / f"{NAME}.zip"
    if not archive.exists():
        # Create-new: concurrent/partial bootstrap is refused rather than replaced.
        with urllib.request.urlopen(URL, timeout=120) as source, archive.open("xb") as target:
            size = 0
            while block := source.read(256 * 1024):
                size += len(block)
                if size > ARCHIVE_SIZE:
                    raise ValueError("toolchain download exceeded pinned size")
                target.write(block)
    if archive.stat().st_size != ARCHIVE_SIZE or digest(archive) != ARCHIVE_SHA256:
        raise ValueError("toolchain archive differs from reviewed digest; inspect/remove the incomplete local copy explicitly")
    with zipfile.ZipFile(archive) as source:
        names = set()
        for info in source.infolist():
            parts = PurePosixPath(info.filename).parts
            if (not parts or parts[0] != NAME or ".." in parts or "\\" in info.filename
                    or ":" in info.filename or info.filename.casefold() in names
                    or stat.S_ISLNK(info.external_attr >> 16)):
                raise ValueError("unsafe toolchain archive entry")
            names.add(info.filename.casefold())
            path = root.joinpath(*parts)
            if info.is_dir():
                directory(path)
                continue
            directory(path.parent)
            with source.open(info) as expected:
                if path.exists():
                    if path.is_symlink() or path.stat().st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT:
                        raise ValueError("toolchain entry is not ordinary")
                    with path.open("rb") as actual:
                        while block := expected.read(256 * 1024):
                            if actual.read(len(block)) != block:
                                raise ValueError("extracted toolchain bytes changed")
                        if actual.read(1):
                            raise ValueError("extracted toolchain size changed")
                else:
                    with path.open("xb") as target:
                        while block := expected.read(256 * 1024):
                            target.write(block)
        actual = {p.relative_to(root).as_posix() for p in (root / NAME).rglob("*") if p.is_file()}
        expected = {i.filename for i in source.infolist() if not i.is_dir()}
        if actual != expected:
            raise ValueError("unexpected extracted toolchain files")
    print(root / NAME)


if __name__ == "__main__":
    prepare()
