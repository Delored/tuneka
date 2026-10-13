# Debian package build

This is an experimental Debian package definition for amd64.

## Debian amd64

From the repository root, run:

```sh
sh packaging/debian/build-deb.sh 0.1.0
```

The package is written to `dist/`. It depends on the distribution's Python and Tk packages. On first launch, Tuneka continues to download its own `yt-dlp` and FFmpeg binaries into the current user's data directory, so the installed app does not write into `/usr`.

Install a local test package with:

```sh
sudo apt install ./dist/tuneka_0.1.0_amd64.deb
```
