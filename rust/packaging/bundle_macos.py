#!/usr/bin/env python3
"""Build and verify an ad-hoc-signed bundle; Python is build tooling only."""
import argparse
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SYSTEM_PREFIXES = ('/System/', '/usr/lib/')


def run(*args):
    return subprocess.check_output(args, text=True)


def dependencies(path):
    return [line.strip().split(' (', 1)[0] for line in run('otool', '-L', str(path)).splitlines()[1:]]


def rpaths(path):
    lines = run('otool', '-l', str(path)).splitlines()
    result = []
    for index, line in enumerate(lines):
        if line.strip() == 'cmd LC_RPATH':
            result.append(lines[index + 2].strip().split('path ', 1)[1].split(' (offset ', 1)[0])
    return result


def expand_path(path, origin, executable):
    for prefix, directory in (('@loader_path', origin.parent), ('@executable_path', executable.parent)):
        if path == prefix or path.startswith(prefix + '/'):
            return directory / path[len(prefix):].lstrip('/')
    return Path(path)


def resolve_dependency(dependency, origin, executable, search_paths):
    if dependency.startswith('@rpath/'):
        suffix = dependency[len('@rpath/'):]
        candidates = [directory / suffix for directory in search_paths]
    else:
        candidates = [expand_path(dependency, origin, executable)]
    for candidate in candidates:
        if candidate.is_absolute() and candidate.is_file():
            return candidate.resolve()
    raise SystemExit(f'Unresolved non-system dependency {dependency} in {origin}')


def copy_native_notices(origins, destination):
    """Retain license files shipped in installed Homebrew kegs, when present."""
    records = {}
    for origin in origins:
        for ancestor in origin.parents:
            if ancestor.parent.parent.name != 'Cellar':
                continue
            keg = ancestor
            name = f'{keg.parent.name}-{keg.name}'
            if name in records:
                break
            files = sorted({path for pattern in ('LICENSE*', 'COPYING*', 'NOTICE*')
                            for path in keg.glob(pattern) if path.is_file()})
            target = destination / name
            target.mkdir(parents=True, exist_ok=True)
            for path in files:
                shutil.copy2(path, target / path.name)
            # Keep the installed formula and receipt to identify sources/options.
            receipt = keg / 'INSTALL_RECEIPT.json'
            if receipt.is_file():
                shutil.copy2(receipt, target / receipt.name)
            formula = keg / '.brew'
            if formula.is_dir():
                shutil.copytree(formula, target / 'formula', dirs_exist_ok=True)
            records[name] = [path.name for path in files]
            break
    return records


def verify_bundle(app, binaries, libraries, architecture):
    contents = app / 'Contents'
    for binary in [*binaries, *libraries]:
        architectures = run('lipo', '-archs', str(binary)).split()
        if architecture not in architectures:
            raise SystemExit(f'{binary.name} lacks architecture {architecture}: {architectures}')
        for dependency in dependencies(binary):
            if dependency.startswith(SYSTEM_PREFIXES):
                continue
            if binary.parent == contents / 'Frameworks' and dependency == '@rpath/' + binary.name:
                continue  # The dylib's own identity is not a loaded dependency.
            resolved = resolve_dependency(dependency, binary, binaries[0], [])
            if not resolved.is_relative_to(contents.resolve()):
                raise SystemExit(f'External dependency remains: {dependency} in {binary}')
        subprocess.check_call(['codesign', '--verify', '--strict', str(binary)])
    subprocess.check_call(['codesign', '--verify', '--deep', '--strict', str(app)])
    environment = {key: value for key, value in os.environ.items() if not key.startswith('DYLD_')}
    subprocess.check_call([str(contents / 'MacOS' / 'tuneka'), '--version'], env=environment)


def sign_bundle(app, binaries, libraries):
    # codesign recognizes the CFBundleExecutable as the enclosing app. Its
    # nested helpers/libraries must already be signed, even for ad-hoc bundles.
    for library in libraries:
        subprocess.check_call(['codesign', '--force', '--sign', '-', str(library)])
    with (app / 'Contents' / 'Info.plist').open('rb') as stream:
        executable = plistlib.load(stream)['CFBundleExecutable']
    main_binary = app / 'Contents' / 'MacOS' / executable
    for binary in binaries:
        if binary != main_binary:
            subprocess.check_call(['codesign', '--force', '--sign', '-', str(binary)])
    subprocess.check_call(['codesign', '--force', '--sign', '-', str(app)])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--profile', choices=['debug', 'release'], default='debug')
    args = parser.parse_args()
    if os.uname().sysname != 'Darwin':
        raise SystemExit('Run on macOS; this is not cross-platform runtime validation.')
    metadata = json.loads(run('cargo', 'metadata', '--locked', '--offline', '--no-deps',
                              '--format-version', '1', '--manifest-path', str(ROOT / 'Cargo.toml')))
    version = next(package['version'] for package in metadata['packages'] if package['name'] == 'tuneka-gui')
    bundle_version = version.split('-', 1)[0].split('+', 1)[0]
    architecture = os.uname().machine
    out = ROOT / 'dist' / f'macos-{architecture}-{args.profile}'
    if out.exists():
        raise SystemExit(f'Output already exists: {out}; choose/remove that generated bundle explicitly.')
    executables = ['tuneka', 'tuneka-tui', 'tuneka-mcp', 'tuneka-gui']
    for name in executables:
        if not (ROOT / 'target' / args.profile / name).is_file():
            raise SystemExit(f'Build {name} first (cargo build --workspace --locked).')
    app = out / 'Tuneka.app' / 'Contents'
    bin_dir = app / 'MacOS'
    libs = app / 'Frameworks'
    bin_dir.mkdir(parents=True)
    libs.mkdir()
    copied = {}
    queue = []
    binaries = []
    for name in executables:
        origin = ROOT / 'target' / args.profile / name
        target = bin_dir / name
        shutil.copy2(origin, target)
        binaries.append(target)
        queue.append((target, origin, origin, []))
    while queue:
        binary, origin, executable, inherited_paths = queue.pop()
        search_paths = [expand_path(path, origin, executable) for path in rpaths(origin)] + inherited_paths
        for dep in dependencies(binary):
            if dep.startswith(SYSTEM_PREFIXES):
                continue
            if dep == '@rpath/' + binary.name and binary.parent == libs:
                continue
            original = resolve_dependency(dep, origin, executable, search_paths)
            if original == origin.resolve():
                continue
            target = libs / original.name
            if target.name in copied and copied[target.name] != original:
                raise SystemExit(f'Library basename collision: {target.name}')
            if target.name not in copied:
                shutil.copy2(original, target)
                target.chmod(target.stat().st_mode | 0o200)
                copied[target.name] = original
                subprocess.check_call(['install_name_tool', '-id', '@rpath/' + target.name, str(target)])
                queue.append((target, original, executable, search_paths))
            replacement = '@loader_path/' + os.path.relpath(target, binary.parent)
            subprocess.check_call(['install_name_tool', '-change', dep, replacement, str(binary)])
    with (app / 'Info.plist').open('wb') as stream:
        plistlib.dump({
            'CFBundleExecutable': 'tuneka-gui', 'CFBundleIdentifier': 'dev.tuneka.rust',
            'CFBundleName': 'Tuneka', 'CFBundleVersion': bundle_version,
            'CFBundleShortVersionString': bundle_version, 'CFBundlePackageType': 'APPL',
            'NSHighResolutionCapable': True,
        }, stream)
    sign_bundle(app.parent, binaries, list(libs.iterdir()))
    verify_bundle(app.parent, binaries, list(libs.iterdir()), architecture)
    shutil.copy2(ROOT / 'THIRD_PARTY_NOTICES.md', out)
    shutil.copytree(ROOT / 'licenses', out / 'licenses', dirs_exist_ok=True)
    native_notices = copy_native_notices(copied.values(), out / 'licenses' / 'native')
    shutil.copy2(ROOT.parent / 'LICENSE', out)
    (out / 'libraries.json').write_text(json.dumps({k: str(v) for k, v in copied.items()}, indent=2))
    (out / 'native-notices.json').write_text(json.dumps(native_notices, indent=2))
    (out / 'LOCAL-DEVELOPMENT-ONLY.txt').write_text('Ad-hoc signed validation bundle. Not notarized. Installed Homebrew native-library licenses and build receipts are retained where available; complete corresponding-source obligations must be fulfilled before release distribution.\n')
    print(out)


if __name__ == '__main__':
    main()
