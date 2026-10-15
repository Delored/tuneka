# Debian package build

This is an experimental Debian package definition for amd64 and arm64. Build it on a matching Debian or Ubuntu system; cross-compilation is not configured.

## Debian amd64 or arm64

From the repository root, run:

```sh
sh packaging/debian/build-deb.sh 0.1.0
```

The package is written to `dist/` with the host architecture in its filename (for example, `tuneka_0.1.0_amd64.deb` or `tuneka_0.1.0_arm64.deb`). It depends on the distribution's Python and Tk packages. On first launch, Tuneka continues to download its own `yt-dlp` and FFmpeg binaries into the current user's data directory, so the installed app does not write into `/usr`.

Install a local test package with:

```sh
sudo apt install ./dist/tuneka_0.1.0_amd64.deb
```

## GitHub Actions

Pull requests targeting `master` run the Python unit and UI regression tests. Package builds are intentionally skipped on pull requests; after a successful merge, the `master` push builds `.deb` packages for both architectures, a Windows x64 installer, and a Windows x64 portable ZIP.

To publish a release, open **Actions → CI and release → Run workflow** on the `master` branch and provide a version such as `v0.2.0`. The workflow reruns tests and builds all packages, then attaches the `.deb` files, Windows installer, and Windows portable ZIP to a GitHub Release. A workflow dispatch from any other branch will not publish a release. Both Windows builds download `yt-dlp` and FFmpeg on first launch into the current user's local application data directory. The installer installs the app under Program Files, creates a Start menu shortcut, and optionally creates a desktop shortcut.
