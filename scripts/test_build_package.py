"""Pure package-policy tests; no registry, browser, native launch or full package build."""
import importlib.util
from pathlib import Path
import struct
import tempfile
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location("package_builder", Path(__file__).with_name("build-package.py"))
BUILDER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILDER)


class PackagePolicy(unittest.TestCase):
    def test_archives_are_deterministic_and_ignore_file_timestamps(self):
        with tempfile.TemporaryDirectory(prefix="dm zip ") as directory:
            root = Path(directory)
            (root / "alpha").write_bytes(b"synthetic bytes")
            (root / "beta").write_bytes(b"other bytes")
            BUILDER.archive(root / "first.zip", {"b": root / "beta", "a": root / "alpha"}, 1700000000)
            BUILDER.archive(root / "second.zip", {"a": root / "alpha", "b": root / "beta"}, 1700000000)
            self.assertEqual((root / "first.zip").read_bytes(), (root / "second.zip").read_bytes())
            with zipfile.ZipFile(root / "first.zip") as archive:
                self.assertEqual(archive.namelist(), ["a", "b"])
                self.assertEqual(archive.read("a"), b"synthetic bytes")
            with self.assertRaises(FileExistsError):
                BUILDER.archive(root / "first.zip", {}, 1700000000)

    def test_archive_traversal_and_nonartifact_output_are_refused(self):
        with tempfile.TemporaryDirectory(prefix="dm zip ") as directory:
            root = Path(directory)
            (root / "source").write_bytes(b"source")
            with self.assertRaises(ValueError):
                BUILDER.archive(root / "bad.zip", {"../outside": root / "source"}, 1700000000)
            with self.assertRaises(ValueError):
                BUILDER.safe_output(root / "not workspace artifacts")
            self.assertFalse((root / "not workspace artifacts").exists())

    def test_runtime_objects_require_deliberate_review(self):
        with tempfile.TemporaryDirectory(prefix="dm runtime ") as directory:
            path = Path(directory) / "synthetic.map"
            path.write_text(" lib64_libmingw32_a-gccmain.o:(.text)\n")
            self.assertEqual(BUILDER.runtime_objects(path), ["lib64_libmingw32_a-gccmain.o"])
            path.write_text(" lib64_libmingwex_a-unreviewed-math.o:(.text)\n")
            with self.assertRaises(ValueError):
                BUILDER.runtime_objects(path)

    def test_pe_inventory_refuses_wrong_architecture_and_delayed_imports(self):
        with tempfile.TemporaryDirectory(prefix="dm pe ") as directory:
            path = Path(directory) / "synthetic.exe"
            data = bytearray(1024)
            data[:2] = b"MZ"
            struct.pack_into("<I", data, 0x3C, 128)
            data[128:132] = b"PE\0\0"
            struct.pack_into("<HH", data, 132, 0x8664, 1)
            struct.pack_into("<H", data, 148, 240)
            struct.pack_into("<H", data, 152, 0x20B)
            path.write_bytes(data)
            self.assertEqual(BUILDER.pe_imports(path), [])
            struct.pack_into("<H", data, 132, 0x14C)
            path.write_bytes(data)
            with self.assertRaises(ValueError):
                BUILDER.pe_imports(path)
            struct.pack_into("<H", data, 132, 0x8664)
            struct.pack_into("<I", data, 152+112+13*8, 4096)
            path.write_bytes(data)
            with self.assertRaises(ValueError):
                BUILDER.pe_imports(path)


if __name__ == "__main__":
    unittest.main()
