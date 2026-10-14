"""Native signing regression; executes real Mach-O tools, not mocked codesign."""
import importlib.util
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    "bundle_macos", Path(__file__).resolve().parents[1] / "bundle_macos.py")
bundle = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bundle)


@unittest.skipUnless(sys.platform == "darwin", "requires macOS Mach-O tools")
class BundleSigningTests(unittest.TestCase):
    def test_unsigned_helpers_and_library_are_signed_before_app(self):
        for tool in ("clang", "codesign", "otool", "lipo"):
            self.assertIsNotNone(shutil.which(tool), f"missing required macOS tool: {tool}")
        with tempfile.TemporaryDirectory(prefix="tuneka-signing-") as temporary:
            root = Path(temporary)
            app = root / "Tuneka.app"
            binaries_dir = app / "Contents" / "MacOS"
            libraries_dir = app / "Contents" / "Frameworks"
            binaries_dir.mkdir(parents=True)
            libraries_dir.mkdir()
            source = root / "library.c"
            source.write_text("int test_value(void) { return 0; }\n", encoding="utf-8")
            library = libraries_dir / "libtest.dylib"
            subprocess.run(["clang", "-dynamiclib", str(source), "-o", str(library),
                            "-Wl,-install_name,@rpath/libtest.dylib"], check=True)
            source = root / "main.c"
            source.write_text('#include <stdio.h>\nint test_value(void);\n'
                              'int main(void) { puts("tuneka signing regression"); '
                              'return test_value(); }\n', encoding="utf-8")
            original = root / "program"
            subprocess.run(["clang", str(source), str(library), "-o", str(original),
                            "-Wl,-rpath,@executable_path/../Frameworks"], check=True)
            binaries = [binaries_dir / name for name in
                        ("tuneka", "tuneka-tui", "tuneka-mcp", "tuneka-gui")]
            for binary in binaries:
                shutil.copy2(original, binary)
            # ARM linkers normally ad-hoc sign; Intel need not. Remove signatures
            # explicitly so both architectures exercise the actual CI failure.
            for binary in [*binaries, library]:
                subprocess.run(["codesign", "--force", "--sign", "-", str(binary)],
                               check=True, capture_output=True)
                subprocess.run(["codesign", "--remove-signature", str(binary)], check=True)
                self.assertNotEqual(subprocess.run(
                    ["codesign", "--verify", str(binary)], capture_output=True).returncode, 0)
            with (app / "Contents" / "Info.plist").open("wb") as stream:
                plistlib.dump({"CFBundleExecutable": "tuneka-gui", "CFBundleName": "Tuneka",
                               "CFBundleIdentifier": "dev.tuneka.signing-test",
                               "CFBundlePackageType": "APPL"}, stream)
            bundle.sign_bundle(app, binaries, [library])
            subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
            for binary in [*binaries, library]:
                subprocess.run(["codesign", "--verify", "--strict", str(binary)], check=True)
            for binary in binaries:
                result = subprocess.run([str(binary), "--version"], check=True,
                                        capture_output=True, text=True, encoding="utf-8")
                self.assertEqual(result.stdout.strip(), "tuneka signing regression")


if __name__ == "__main__":
    unittest.main()
