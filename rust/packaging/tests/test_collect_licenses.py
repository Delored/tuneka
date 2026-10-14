import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "collect_licenses.py"
SPEC = importlib.util.spec_from_file_location("collect_licenses", SCRIPT)
collector = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(collector)


class CollectLicensesEncodingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="tuneka-license-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.output = self.root / "collected"
        self.package = self.root / "dependency"
        self.package.mkdir()
        self.license_text = "Copyright Євген Ёж\nPermission is hereby granted.\n".encode("utf-8")
        self.license_name = "Ліцензія-Ё.txt"
        (self.package / self.license_name).write_bytes(self.license_text)
        (self.package / "Cargo.toml").write_text("", encoding="utf-8")
        (self.root / "Cargo.lock").write_text("# fixture lockfile\n", encoding="utf-8")
        (self.root / "THIRD_PARTY_NOTICES.md").write_text("Fixture notices\n", encoding="utf-8")
        (self.root / "LICENSE").write_text("Fixture project license\n", encoding="utf-8")
        (self.root / "licenses").mkdir()
        (self.root / "licenses" / "protocol.txt").write_bytes(b"Fixture protocol license\n")
        self.metadata = {
            "resolve": {"nodes": [{"id": "fixture-id"}]},
            "packages": [{
                "id": "fixture-id", "name": "fixture", "version": "1.0.0",
                "source": None, "license": "MIT", "license_file": self.license_name,
                "manifest_path": str(self.package / "Cargo.toml"),
                "description": "UTF-8 dependency metadata: Ё",
            }],
        }

    def collect(self, metadata_bytes):
        real_check_output = subprocess.check_output
        rustc_bytes = b"rustc 1.96.0\nhost: x86_64-pc-windows-msvc\n"

        def run_fixture_command(command, **kwargs):
            if command[0] == "rustc":
                data = rustc_bytes
            elif command[0] == "cargo":
                data = metadata_bytes
            else:
                self.fail(f"Unexpected command: {command!r}")
            # Execute a real child process and let subprocess decode its stdout.
            # Only the external tool implementation is replaced with fixture bytes.
            return real_check_output(
                [sys.executable, "-c", "import sys; sys.stdout.buffer.write(" + repr(data) + ")"],
                **kwargs,
            )

        argv = [str(SCRIPT), str(self.output), "--project-license", str(self.root / "LICENSE")]
        with mock.patch.object(collector, "ROOT", self.root), \
                mock.patch.object(sys, "argv", argv), \
                mock.patch.object(subprocess, "check_output", side_effect=run_fixture_command), \
                mock.patch.object(subprocess, "_text_encoding", return_value="cp1252"):
            # Force Windows' locale fallback even on UTF-8 Linux/macOS/Python.
            # An explicit command encoding bypasses this fallback naturally.
            collector.main()

    def test_utf8_metadata_with_cp1252_locale_copies_unicode_license(self):
        metadata_bytes = json.dumps(self.metadata, ensure_ascii=False).encode("utf-8")
        self.assertIn(b"\x81", metadata_bytes)  # Undefined in the failed Windows cp1252 decoder.
        self.collect(metadata_bytes)
        catalog = json.loads((self.output / "catalog.json").read_text(encoding="utf-8"))
        self.assertEqual(catalog["target"], "x86_64-pc-windows-msvc")
        package, = catalog["packages"]
        self.assertEqual((package["name"], package["license"]), ("fixture", "MIT"))
        copied, = package["files"]
        self.assertEqual(Path(copied["path"]).name, self.license_name)
        self.assertEqual((self.output / copied["path"]).read_bytes(), self.license_text)
        self.assertEqual(copied["sha256"], hashlib.sha256(self.license_text).hexdigest())
        self.assertEqual((self.output / "Tuneka-LICENSE").read_bytes(), b"Fixture project license\n")
        self.assertTrue((self.output / "README.txt").read_text(encoding="utf-8"))

    def test_invalid_utf8_metadata_is_rejected_before_creating_catalog(self):
        self.metadata["packages"][0]["license_file"] = None
        self.metadata["packages"][0]["description"] = "invalid-byte-marker"
        metadata_bytes = json.dumps(self.metadata).encode("ascii").replace(b"invalid-byte-marker", b"\xff")
        with self.assertRaises(UnicodeDecodeError):
            self.collect(metadata_bytes)
        self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
