"""Bounded metadata and create-new Windows evidence sinks, not product path policy."""
from contextlib import contextmanager
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import re
import uuid

LIMIT = 64 * 1024
ARTIFACTS = Path(__file__).resolve().parents[2] / "artifacts"


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate metadata member")
        result[key] = value
    return result


def invalid_constant(_):
    raise ValueError("invalid metadata number")


def bounded_json(path):
    with path.open("rb") as source:
        data = source.read(LIMIT + 1)
    if len(data) > LIMIT:
        raise RuntimeError("oversized owned metadata")
    try:
        return json.loads(data.decode("utf-8"), object_pairs_hook=unique_object, parse_constant=invalid_constant)
    except (ValueError, RecursionError):
        raise RuntimeError("invalid owned metadata") from None


def new_report(path):
    # Check spelling before resolve: resolving can hide aliases, devices or streams.
    path = path.absolute()
    try:
        parts = path.relative_to(ARTIFACTS).parts
    except ValueError:
        raise RuntimeError("report must be beneath artifacts") from None
    reserved = {"con", "prn", "aux", "nul", *(f"{prefix}{digit}" for prefix in ("com", "lpt") for digit in "123456789")}
    if not parts or any(not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._ -]{0,127}", part)
                        or part.endswith((".", " ")) or part.split(".")[0].lower() in reserved for part in parts):
        raise RuntimeError("unsafe report spelling; use ordinary Windows-safe ASCII names")
    if any(parent.is_symlink() or parent.is_junction() for parent in (path, *path.parents)) or path.resolve() != path:
        raise RuntimeError("report aliases are not authorized")
    if any(parent.exists() and not parent.is_dir() for parent in path.parents):
        raise RuntimeError("report parent is not an ordinary directory")
    if path.exists() or path.suffix != ".json":
        raise RuntimeError("use a new JSON report beneath artifacts")
    return path


@contextmanager
def leased_parent(path):
    """Block ordinary ancestor rename/deletion while publishing, not hostile-account isolation."""
    kernel = ctypes.WinDLL(str(Path(os.environ["WINDIR"]) / "System32/kernel32.dll"), use_last_error=True)
    kernel.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p,
                                  wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    kernel.CreateFileW.restype = wintypes.HANDLE
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.CloseHandle.restype = wintypes.BOOL
    kernel.GetFileInformationByHandleEx.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
    kernel.GetFileInformationByHandleEx.restype = wintypes.BOOL
    class AttributeTag(ctypes.Structure):
        _fields_ = [("attributes", wintypes.DWORD), ("tag", wintypes.DWORD)]
    handles = []
    try:
        for parent in reversed(path.parents):
            parent.mkdir(exist_ok=True)  # Its ancestors are already leased.
            # Metadata-only access (0) does NOT deny directory renames on Windows.
            # GENERIC_READ participates in sharing checks, matching setup's lease.
            handle = kernel.CreateFileW(str(parent), 0x80000000, 3, None, 3, 0x02200000, None)
            if handle == ctypes.c_void_p(-1).value:
                raise RuntimeError("report directory lease refused")
            handles.append(handle)
            info = AttributeTag()
            if (not kernel.GetFileInformationByHandleEx(handle, 9, ctypes.byref(info), ctypes.sizeof(info))
                    or not info.attributes & 0x10 or info.attributes & 0x400):
                raise RuntimeError("report directory is not ordinary")
        yield
    finally:
        for handle in reversed(handles):
            kernel.CloseHandle(handle)


def write_report(path, evidence):
    if os.name != "nt":
        raise RuntimeError("evidence publication requires Windows no-replace rename semantics")
    path = new_report(path)
    data = (json.dumps(evidence, indent=2, allow_nan=False) + "\n").encode("utf-8")
    if len(data) > LIMIT:
        raise RuntimeError("oversized evidence report")
    with leased_parent(path):
        new_report(path)
        temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}.partial")
        # Failed writes/renames preserve this owned partial rather than claiming a report.
        with temporary.open("xb") as target:
            target.write(data)
            target.flush()
            os.fsync(target.fileno())
        os.rename(temporary, path)  # Windows refuses an existing destination, including links.
