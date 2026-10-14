#!/usr/bin/env python3
"""Collect the native target's Cargo license texts/catalog for build artifacts."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("destination", type=Path)
    parser.add_argument("--project-license", type=Path, default=ROOT.parent / "LICENSE")
    args = parser.parse_args()
    output = args.destination.resolve()
    if output.exists():
        raise SystemExit(f"License output already exists: {output}")
    rustc = subprocess.check_output(["rustc", "-vV"], text=True, cwd=ROOT)
    host = next(line.split(": ", 1)[1] for line in rustc.splitlines() if line.startswith("host: "))
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--offline", "--format-version", "1",
         "--filter-platform", host], text=True, cwd=ROOT))
    resolved = {node["id"] for node in metadata["resolve"]["nodes"]}
    output.mkdir(parents=True)
    shutil.copy2(ROOT / "Cargo.lock", output / "Cargo.lock")
    shutil.copy2(ROOT / "THIRD_PARTY_NOTICES.md", output / "THIRD_PARTY_NOTICES.md")
    shutil.copy2(args.project_license, output / "Tuneka-LICENSE")
    shutil.copytree(ROOT / "licenses", output / "protocols")
    packages = []
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if package["id"] not in resolved:
            continue
        directory = Path(package["manifest_path"]).parent.resolve()
        candidates = []
        for child in directory.iterdir():
            if child.name.lower().startswith(("license", "licence", "copying", "notice", "unlicense")):
                candidates.extend(child.rglob("*") if child.is_dir() else [child])
        if package.get("license_file"):
            candidates.append(directory / package["license_file"])
        files = []
        suffix = hashlib.sha256((package.get("source") or "workspace").encode()).hexdigest()[:8]
        package_dir = Path("dependencies") / f"{package['name']}-{package['version']}-{suffix}"
        for candidate in sorted(set(candidates)):
            source = candidate.resolve()
            # Never copy outside a dependency directory, or copy large/binary assets.
            if not source.is_file() or not source.is_relative_to(directory) or source.stat().st_size > 2_000_000:
                continue
            data = source.read_bytes()
            if b"\0" in data:
                continue
            destination = package_dir / source.relative_to(directory)
            (output / destination).parent.mkdir(parents=True, exist_ok=True)
            (output / destination).write_bytes(data)
            files.append({"path": destination.as_posix(), "sha256": hashlib.sha256(data).hexdigest()})
        packages.append({"name": package["name"], "version": package["version"],
                         "license": package.get("license"), "source": package.get("source"),
                         "files": files})
    (output / "catalog.json").write_text(json.dumps({"target": host, "packages": packages}, indent=2) + "\n")
    (output / "README.txt").write_text(
        "License texts shipped by resolved native-target Cargo packages, including build/dev dependencies.\n"
        "catalog.json records license expressions and copied files; an empty files list needs review.\n"
        "This catalog is not a completed release license/source-compliance audit. Native library notices\n"
        "are collected separately by each platform packager. Cargo.lock records exact dependency versions.\n")
    print(f"Collected license catalog for {len(packages)} packages ({host})")


if __name__ == "__main__":
    main()
