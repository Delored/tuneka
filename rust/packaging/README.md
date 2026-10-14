# CI artifacts, local packaging and platform validation

All scripts are build/validation tooling. They do not become product runtime dependencies and do not publish releases.

## GitHub Actions artifacts

`.github/workflows/rust-artifacts.yml` builds release-profile artifacts for all five targets in the first-release plan. It runs on relevant pushes to `master` / `rust-version`, pull requests targeting those branches, and manual **Actions → Rust artifacts → Run workflow**. Once the workflow exists on the default branch, the manual selector can run another branch containing the workflow. Download the successful platform jobs from the run's **Artifacts** section; each artifact is retained for 14 days. A failed platform does not cancel the other platform jobs.

| Artifact | Native runner | Contents |
|---|---|---|
| `tuneka-rust-linux-amd64` | Ubuntu 24.04 x64, Debian 13 build container | Headless and GUI `.deb` packages, SHA-256, source revision, Cargo license catalog |
| `tuneka-rust-linux-arm64` | Ubuntu 24.04 ARM64, Debian 13 build container | Same packages for ARM64 |
| `tuneka-rust-macos-arm64` | macOS 15 Apple Silicon | `.tar.gz` containing ad-hoc-signed `Tuneka.app`, all four executables, native libraries and notices; SHA-256 |
| `tuneka-rust-macos-x86_64` | macOS 15 Intel | Same bundle for Intel |
| `tuneka-rust-windows-x64` | Windows Server 2022 / MSVC x64 | Portable ZIP with all four `.exe` files, FFmpeg/native DLLs, app-local VC runtime, notices, import/validation manifests and SHA-256 |

All builds use the checked-in Rust 1.96 toolchain and Cargo.lock. Linux uses Debian FFmpeg 7; macOS uses Homebrew `ffmpeg@7`; Windows builds FFmpeg 7.1.1 from the pinned vcpkg registry in [windows/README.md](windows/README.md). Unit/integration tests must pass before upload. Linux additionally installs the actual packages in a fresh Debian container and checks CLI startup and native dependencies; macOS verifies its bundled dependency graph/signatures and CLI startup; Windows verifies PE imports and portable CLI/MCP startup with development paths removed. These checks do not claim GUI interaction on all hosted runners; the separate `rust.yml` workflow retains its X11/TUI validation.

The pipeline requires no signing secrets, write permissions or release tokens. Actions are pinned by commit. It uploads development CI artifacts, without creating tags or GitHub releases. macOS artifacts are not notarized and Windows artifacts are not Authenticode signed. Cargo/native license files and build/source metadata are retained; a complete redistributable source/license audit remains a release gate. Windows ARM64 remains deferred in the implementation plan.

Local validation of this pipeline: `actionlint`, Dockerfile build checks, Python/PowerShell/shell parsing, a native macOS test bundle with transitive dylibs/signing, and actual Cargo license collection passed. Full five-platform Actions execution has not been run from this checkout; push the workflow and Rust source to make those CI results available. Existing earlier platform runtime results are recorded separately below.

## Local commands

macOS: build `cargo build --workspace --locked`, then `python3 packaging/bundle_macos.py --profile debug`. A locally ad-hoc-signed Tuneka.app contains all four executables and the FFmpeg native-library closure. This is a development artifact; notarization and complete redistributable notices/sources are separate release gates. The builder refuses to overwrite an existing bundle.

Linux commands (run from `rust/`):

```sh
docker build --target headless -f packaging/Dockerfile.linux -t tuneka-rust-local:arm64 .
docker run --rm --network none tuneka-rust-local:arm64
docker build --target desktop-deb-export -f packaging/Dockerfile.linux --output type=local,dest=dist/linux-arm64 .
docker build --platform linux/amd64 --target desktop-deb-export -f packaging/Dockerfile.linux --output type=local,dest=dist/linux-amd64 .
# Native release artifacts, as used in CI:
docker build --build-arg BUILD_PROFILE=release --build-arg BUILD_JOBS=2 --build-arg PACKAGE_VERSION=0.1.0 --target desktop-deb-export -f packaging/Dockerfile.linux --output type=local,dest=dist/linux-release .
```

The first architecture follows the host; ARM64 names above assume an ARM64 host. Use `--platform linux/arm64` explicitly on other hosts. Cross-architecture Docker execution is emulated, not physical hardware coverage. The image uses Debian 13/FFmpeg 7, locked dependencies and defaults to four build jobs; CI sets `BUILD_JOBS=2`. Architecture/profile-specific Cargo cache mounts keep build intermediates out of final runtime layers. GUI build dependencies are cached separately from application source. Keep several GiB free for builds; do not globally prune other projects' images/volumes to make room.

Two independent development packages are exported: `tuneka-rust` (CLI/TUI/MCP) and `tuneka-rust-gui` (GPUI plus desktop entry). On a matching Debian 13 system, install the selected `.deb` with `sudo apt install ./path/to/package.deb`; apt resolves runtime dependencies. The GUI requires an X11/Wayland session and a Vulkan driver. These packages use distribution FFmpeg libraries rather than embedding a second copy. The builder refuses to overwrite an existing package in its own staging directory.

Software-rendered X11 validation:

```sh
docker build --target gui-smoke -f packaging/Dockerfile.linux -t tuneka-gui-smoke .
mkdir -p artifacts/linux-gui
docker run --rm --network none --mount "type=bind,src=$PWD/artifacts/linux-gui,dst=/evidence" --mount "type=bind,src=$PWD/packaging,dst=/checks,readonly" tuneka-gui-smoke sh /checks/smoke_linux_gui.sh
```

This opens a real GPUI window under Xvfb/Openbox using Mesa Vulkan, enters an invalid URL, checks resizing and graceful quit, and captures XWD evidence. It is not a physical-GPU or Wayland test. Openbox is required for reliable window presentation/focus. Docker and smoke tools are build/validation infrastructure, not installed product runtime dependencies. See `../docs/validation.md` for actual test and clean-container installation results.

Windows x64 packaging is implemented in `windows/build.ps1`; it requires Visual Studio C++/SDK, LLVM, Rust and Python as documented in `windows/README.md`. CI provisions native Windows x64 and macOS x64 builds; their actual successful execution must still be confirmed by a workflow run. No cross-target build or runtime success should be inferred from current macOS ARM64 or Linux container checks. Windows ARM64 is deferred per the implementation plan.

The Python distribution workflow remains intact. Separate `rust.yml` validation and `rust-artifacts.yml` packaging workflows follow the plan's explicit CI exception; neither was pushed or run remotely during this task. Local packages are not published releases; complete notices/source obligations and the remaining platform matrix must be checked before release distribution.

Validated locally on 2026-10-14: ARM64 and emulated amd64 build/test/package, clean Debian 13 package install/uninstall, unprivileged CLI and X11/Openbox software-Vulkan GUI input/resize/quit. ARM64 additionally completed real CLI and GUI downloads. See `../docs/validation.md` for exact evidence and remaining platform/release gates.
