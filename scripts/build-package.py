"""Explicit allowlist packaging; no checkout/archive/profile directory is copied wholesale."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import platform
import zlib
import shutil
import struct
import subprocess
import time
import zipfile

ROOT = Path(__file__).resolve().parent.parent
TOOLCHAIN = ROOT / "target/toolchains/llvm-mingw-20260826-ucrt-x86_64"
TOOLCHAIN_SHA256 = "ae601f4e0f72bbdf441ad2df8bb16f037e2e9251559ea6b37b4057aef39c06c3"
PAYLOADS = ["download-manager-native-host.exe", "download-manager-setup.exe",
            "firefox-download-manager.xpi", "INSTALL.md", "SECURITY.md",
            "THIRD-PARTY-NOTICES.txt", "BUILD-INFO.json", "LICENSE.txt"]
EXTENSION = {"background.js", "background.js.map", "manager.js", "manager.js.map",
             "manifest.json", "manager.html", "manager.css", "LICENSE.txt", "THIRD-PARTY-NOTICES.txt"}
SYSTEM_DLLS = {"advapi32.dll", "bcrypt.dll", "bcryptprimitives.dll", "crypt32.dll", "dbghelp.dll", "gdi32.dll",
               "iphlpapi.dll", "kernel32.dll", "mswsock.dll", "ncrypt.dll", "ntdll.dll",
               "ole32.dll", "oleaut32.dll", "secur32.dll", "shell32.dll", "userenv.dll",
               "user32.dll", "version.dll", "winhttp.dll", "wintrust.dll", "ws2_32.dll",
               "normaliz.dll", "rpcrt4.dll", "ucrtbase.dll", "psapi.dll", "winmm.dll"}

SYSTEM_DLLS.update({'api-ms-win-crt-stdio-l1-1-0.dll', 'api-ms-win-crt-runtime-l1-1-0.dll', 'api-ms-win-crt-math-l1-1-0.dll', 'api-ms-win-crt-locale-l1-1-0.dll', 'api-ms-win-crt-convert-l1-1-0.dll', 'api-ms-win-crt-utility-l1-1-0.dll', 'api-ms-win-crt-private-l1-1-0.dll', 'api-ms-win-crt-environment-l1-1-0.dll', 'api-ms-win-core-synch-l1-2-0.dll', 'api-ms-win-crt-string-l1-1-0.dll', 'api-ms-win-crt-heap-l1-1-0.dll', 'api-ms-win-crt-filesystem-l1-1-0.dll'})


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True, encoding="utf-8").strip()


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def ordinary(path):
    if path.is_symlink() or getattr(path.lstat(), "st_file_attributes", 0) & 0x400:
        raise ValueError("reparse package input/output refused")
    if not path.is_file():
        raise ValueError("non-file package input refused")
    return path


def safe_output(path):
    path = Path(os.path.abspath(path))
    if not path.is_relative_to(ROOT / "artifacts") or path == ROOT / "artifacts":
        raise ValueError("package output must be a new artifacts subdirectory")
    for parent in [path, *path.parents]:
        if parent.exists() and (parent.is_symlink() or getattr(parent.lstat(), "st_file_attributes", 0) & 0x400):
            raise ValueError("reparse output ancestor refused")
    path.mkdir(parents=True, exist_ok=False)
    return path


def archive(target, entries, epoch):
    stamp = time.gmtime(max(315532800, epoch))[:6]
    with zipfile.ZipFile(target, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as output:
        for name, path in sorted(entries.items()):
            if Path(name).is_absolute() or ".." in Path(name).parts or "\\" in name:
                raise ValueError("unsafe archive member")
            entry = zipfile.ZipInfo(name, stamp)
            entry.create_system = 3
            entry.external_attr = 0o100644 << 16
            entry.compress_type = zipfile.ZIP_DEFLATED
            output.writestr(entry, ordinary(path).read_bytes(), compresslevel=9)


def pe_imports(path):
    """Bounded PE32+ import inventory; never load/execute the inspected DLLs."""
    data = ordinary(path).read_bytes()
    if len(data) > 256 * 1024 * 1024 or data[:2] != b"MZ":
        raise ValueError("invalid executable")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe+4] != b"PE\0\0" or struct.unpack_from("<H", data, pe+4)[0] != 0x8664:
        raise ValueError("package requires an x64 PE")
    sections = struct.unpack_from("<H", data, pe+6)[0]
    size = struct.unpack_from("<H", data, pe+20)[0]
    opt = pe+24
    if struct.unpack_from("<H", data, opt)[0] != 0x20B or not 1 <= sections <= 96:
        raise ValueError("unsupported PE layout")
    table = opt+size

    def offset(rva):
        for index in range(sections):
            base = table+index*40
            virtual_size, address, raw_size, raw = struct.unpack_from("<IIII", data, base+8)
            if address <= rva < address+max(virtual_size, raw_size):
                result = raw+rva-address
                if result >= len(data):
                    break
                return result
        raise ValueError("invalid PE address")

    imports = struct.unpack_from("<I", data, opt+120)[0]
    found = []
    if imports:
        start = offset(imports)
        for index in range(128):
            row = struct.unpack_from("<IIIII", data, start+index*20)
            if not any(row):
                break
            name = offset(row[3])
            end = data.index(0, name, name+256)
            found.append(data[name:end].decode("ascii").lower())
        else:
            raise ValueError("too many PE imports")
    # Delay imports must be deliberately reviewed, not hidden from the inventory.
    if struct.unpack_from("<I", data, opt+112+13*8)[0]:
        raise ValueError("unreviewed delayed imports")
    return sorted(set(found))


def runtime_objects(path):
    policy = json.loads((ROOT / "scripts/runtime-policy.json").read_text(encoding="utf-8"))
    objects = sorted(set(re.findall(r"(lib64_lib[^\s:]+\.o):", ordinary(path).read_text(encoding="utf-8"))))
    if not objects or not set(objects) <= set(policy["mingw_objects"]):
        raise ValueError("unreviewed MinGW runtime object; review license/source before packaging")
    return objects


def notices():
    metadata = json.loads(command("cargo", "metadata", "--locked", "--format-version", "1",
                                  "--filter-platform", "x86_64-pc-windows-gnullvm"))
    packages = {item["id"]: item for item in metadata["packages"]}
    nodes = {item["id"]: item for item in metadata["resolve"]["nodes"]}
    pending = [item["id"] for item in packages.values()
               if item["name"] in ("download-manager-native-host", "download-manager-setup")]
    seen = set()
    while pending:
        key = pending.pop()
        if key in seen:
            continue
        seen.add(key)
        pending.extend(dep["pkg"] for dep in nodes[key]["deps"]
                       if any(kind["kind"] != "dev" for kind in dep["dep_kinds"]))
    text = ["Firefox Download Manager — third-party notices\n",
            "First-party license: MIT; see LICENSE.txt. Third-party terms below remain independent.\n",
            "Dependency runtime/build closure for the Windows x64 helper and setup follows.\n"]
    inventory = []
    for item in sorted((packages[key] for key in seen), key=lambda p: (p["name"], p["version"])):
        if item["source"] is None:
            continue
        root = Path(item["manifest_path"]).parent
        license_files = [file for file in root.rglob("*") if file.is_file() and file.name.upper().startswith(
            ("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE"))]
        if item["license_file"]:
            candidate = (root / item["license_file"]).resolve()
            if not candidate.is_relative_to(root.resolve()):
                raise ValueError("license input escapes its package")
            license_files.append(candidate)
        if not license_files or not item["license"]:
            raise ValueError("dependency lacks reviewed license material")
        text.append(f"\n{'='*72}\n{item['name']} {item['version']} — {item['license']}\n")
        inventory.append({"name": item["name"], "version": item["version"], "license": item["license"]})
        for file in sorted(set(license_files)):
            ordinary(file)
            if file.stat().st_size > 8 * 1024 * 1024:
                raise ValueError("oversized license input")
            text.append(f"\n--- {file.relative_to(root).as_posix()} ---\n{file.read_text(encoding='utf-8')}\n")
    rust = Path(command("rustc", "--print", "sysroot")) / "share/doc/rust"
    text.append("\nRust standard library/toolchain notices (MIT OR Apache-2.0)\n")
    copyright = ordinary(rust / "COPYRIGHT-library.html").read_text(encoding="utf-8")
    text.append("Original library attribution HTML follows; it covers several target platforms, not only this PE.\n")
    text.append(copyright)
    for name in sorted(set(re.findall(r'licenses/([A-Za-z0-9.+_-]+\.txt)', copyright)) | {"MIT.txt", "Apache-2.0.txt"}):
        text.append(f"\n--- Rust {name} ---\n")
        text.append(ordinary(rust / "licenses" / name).read_text(encoding="utf-8"))
    text.append("\nesbuild generated-code/build-tool notice:\n")
    text.append(ordinary(ROOT / "node_modules/esbuild/LICENSE.md").read_text(encoding="utf-8"))
    text.append("\nLLVM/MinGW compiler runtime notices. Static MinGW/LLVM support, system-provided UCRT; no Microsoft runtime object code, DLL or SDK is copied into the package.\n")
    for relative in ["LICENSE.TXT", "x86_64-w64-mingw32/share/mingw32/COPYING", "x86_64-w64-mingw32/share/mingw32/COPYING.MinGW-w64-runtime.txt", "x86_64-w64-mingw32/share/mingw32/COPYING.winpthreads.txt"]:
        text.append(f"\n--- LLVM/MinGW {relative} ---\n")
        text.append(ordinary(TOOLCHAIN / relative).read_text(encoding="utf-8"))
    return "\n".join(text), inventory


def build(binary_dir, output, development=False):
    project = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
    if project.get("license") != "MIT":
        raise ValueError("first-party license metadata requires deliberate review")
    version = project["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?", version):
        raise ValueError("unsafe version")
    dirty = bool(command("git", "status", "--porcelain", "--untracked-files=normal"))
    if dirty and not development:
        raise ValueError("production candidates require a clean source tree")
    if command("git", "remote", "get-url", "origin") not in ("https://github.com/HalcyonXP/firefox-download-manager.git", "https://github.com/HalcyonXP/firefox-download-manager"):
        raise ValueError("package source is not the canonical origin")
    binary_dir = Path(binary_dir).resolve()
    if not binary_dir.is_relative_to((ROOT / "target").resolve()):
        raise ValueError("binary inputs must be explicitly built in the workspace target tree")
    out = safe_output(output)
    epoch = int(command("git", "show", "-s", "--format=%ct", "HEAD"))
    info = {"repository": "HalcyonXP/firefox-download-manager", "commit": command("git", "rev-parse", "HEAD"),
            "package_version": version, "first_party_license": "MIT", "target": "x86_64-pc-windows-gnullvm", "source_dirty": dirty,
            "development": development, "qualification": "Not asserted by builder; see release notes for this exact checksum.",
            "recipe": "development-unqualified" if development else "windows-x64-llvm-ucrt-v1", "rustc": command("rustc", "-V"), "cargo": command("cargo", "-V"),
            "node": command("node", "--version"),
            "npm": command(str(Path(os.environ["WINDIR"]) / "System32/cmd.exe"), "/d", "/c", "npm --version"),
            "cmake": command("cmake", "--version").splitlines()[0], "ninja": command("ninja", "--version"), "python": platform.python_version(), "zlib": zlib.ZLIB_VERSION, "wire_version": 2, "task_state_version": 4,
            "toolchain": {"archive": TOOLCHAIN.name + ".zip", "sha256": TOOLCHAIN_SHA256,
                          "clang": command(str(TOOLCHAIN / "bin/x86_64-w64-mingw32-clang.exe"), "--version").splitlines()[0]},
            "settings_version": 2, "minimum_firefox": "156.0", "imports": {}, "mingw_runtime_objects": {}}
    for name in PAYLOADS[:2]:
        file = ordinary(Path(binary_dir) / name)
        imports = pe_imports(file)
        info["imports"][name] = imports
        info["mingw_runtime_objects"][name] = runtime_objects(ROOT / "target/package-maps" / name.replace(".exe", ".map"))
        if not development and any(dll not in SYSTEM_DLLS  for dll in imports):
            raise ValueError("unreviewed or non-stock runtime dependency")
        shutil.copyfile(file, out / name)
    ext = ROOT / "extension/dist"
    if {file.name for file in ext.iterdir()} != EXTENSION:
        raise ValueError("unexpected extension build inputs")
    manifest = json.loads(ordinary(ext / "manifest.json").read_text(encoding="utf-8"))
    if manifest["version"] != version or manifest["browser_specific_settings"]["gecko"]["strict_min_version"] != "156.0":
        raise ValueError("mixed extension version/policy")
    license_text = ordinary(ROOT / "LICENSE").read_text(encoding="utf-8")
    if ordinary(ext / "LICENSE.txt").read_text(encoding="utf-8") != license_text:
        raise ValueError("extension first-party license differs from source")
    esbuild_license = ordinary(ROOT / "node_modules/esbuild/LICENSE.md").read_text(encoding="utf-8")
    if esbuild_license not in ordinary(ext / "THIRD-PARTY-NOTICES.txt").read_text(encoding="utf-8"):
        raise ValueError("extension lacks reviewed esbuild attribution")
    (out / "LICENSE.txt").write_text(license_text, encoding="utf-8", newline="\n")
    archive(out / PAYLOADS[2], {name: ext / name for name in EXTENSION}, epoch)
    for source, target in [("INSTALLATION.md", "INSTALL.md"), ("PACKAGE_SECURITY.md", "SECURITY.md")]:
        # A clean Git worktree can still have CRLF before index normalization.
        (out / target).write_text(ordinary(ROOT / "docs" / source).read_text(encoding="utf-8"), encoding="utf-8", newline="\n")
    notice, inventory = notices()
    (out / "THIRD-PARTY-NOTICES.txt").write_text(notice, encoding="utf-8", newline="\n")
    info["dependencies"] = inventory
    (out / "BUILD-INFO.json").write_text(json.dumps(info, indent=2)+"\n", encoding="utf-8", newline="\n")
    descriptor = {"format": "firefox-download-manager-package", "version": 1, "package_version": version,
                  "repository": info["repository"], "commit": info["commit"], "target": info["target"],
                  "files": {name: digest(out / name) for name in sorted(PAYLOADS)}}
    (out / "package.json").write_text(json.dumps(descriptor, indent=2)+"\n", encoding="utf-8", newline="\n")
    leaves = [*PAYLOADS, "package.json"]
    (out / "SHA256SUMS.txt").write_text("".join(f"{digest(out / name)}  {name}\n" for name in sorted(leaves)), encoding="utf-8", newline="\n")
    archive_name = f"firefox-download-manager-{version}-windows-x64.zip"
    archive(out / archive_name, {name: out / name for name in [*leaves, "SHA256SUMS.txt"]}, epoch)
    (out / "PACKAGE-SHA256SUMS.txt").write_text(f"{digest(out / archive_name)}  {archive_name}\n", encoding="utf-8", newline="\n")
    print(f"Built checksummed {'development' if development else 'candidate'} package; qualification is separate.")
    return out


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--development", action="store_true")
    args = parser.parse_args()
    build(args.binary_dir, args.output, args.development)
