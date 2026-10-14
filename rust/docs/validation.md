# Validation — 2026-10-14

This is a working local Rust implementation, not a completed cross-platform release. Product changes are under `rust/`, plus the separate `.github/workflows/rust.yml` explicitly allowed by the plan. The Python application and its workflow are unchanged. Generated logs, screenshots, media and packages are ignored by Git.

The latest [M2 CDN follow-up](m2-cdn-followup.md) verifies genuine cipher delivery and long SABR with an explicitly supplied proof; automatic proof acquisition remains open. The preceding M2 continuation adds real finite HLS downloads/resume and current indexed player challenge execution. See [M2 validation](m2-validation.md) for its current evidence, final checks and limitations. Platform packages and baseline counts below describe the earlier M0–M6 continuation; they are not newly packaged M2 binaries.

## Host and offline checks

Host: macOS Apple Silicon, Rust 1.96.0, FFmpeg 7 libraries (`libavcodec 61.19.100`, `libavformat 61.7.100`), GPUI 0.2.2 with `runtime_shaders`, QuickJS/rquickjs 0.14.0. FFmpeg is called through linked libraries; independent QA uses FFmpeg command-line tools.

From `rust/`:

```sh
cargo fmt --all -- --check
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets -- -D warnings
python3 packaging/smoke_tui.py
```

The final complete macOS workspace run passed **59 tests with zero failures and one ignored helper**, including the continuation work below. Linux ARM64 and emulated amd64 also passed the same 55 headless + 4 GUI tests. The final TUI diagnostic-rendering regression adds one passing test (60 covered overall); scoped native build/tests/Clippy and PTY smoke passed after that change. The ignored helper is executed by the real subprocess lock-conflict test. Coverage includes range integrity and retries, persistent locks, publication crash recovery, symlink confinement, partial playlist outcomes, settings, extractor/JS fixtures, real media encoding/decoding, MCP child-process protocol lifecycle and native input/focus regressions. Final macOS formatting, workspace build and Clippy passed. `block 0.1.6`, an upstream GUI dependency, emits a future-compatibility advisory.

Final continuation logs: `../artifacts/tests-workspace-continuation.log`, `clippy-workspace-continuation.log`, `build-workspace-continuation.log`, `linux-arm64-gui-build-final.log`, `tui-pty.log`. Earlier logs remain as historical evidence. The PTY smoke checks actual TUI rendering/input/error/quit and terminal restoration.

Final live frontend checks also pass: `../artifacts/tui-live-logfix/report.md` records real URL → inspect → selected track → completed MP3 → quit, with identical before/after terminal flags. `../artifacts/mcp-live/report.md` records schema discovery, immediate job ID (11.7 ms), resolving/downloading/processing/completed polling, a valid 192.072-second MP3, 26 JSON-RPC stdout messages and EOF shutdown exit 0. Both used explicit isolated Rust state/output directories and bounded execution. The first TUI run exposed native FFmpeg warnings overwriting the terminal. Only the TUI process now disables native FFmpeg logging before worker startup; typed failures still reach job state. A repeated real download confirms zero native warnings in the PTY. The Jobs row now also displays per-item failures/warnings when the aggregate job has no error; a rendered-cell regression covers this. CLI/MCP native diagnostic behavior is unchanged. Scoped logs: `../artifacts/tui-logfix-tests.log`, `tui-diagnostics-tests.log`, `tui-diagnostics-clippy.log`.

## Live pipeline evidence

Public anonymous YouTube checks used the implementation in this checkout on 2026-10-14. These are time-specific observations, not a guarantee that YouTube clients remain stable. No account cookies or signed stream URLs are persisted.

| Scenario | Result and evidence |
|---|---|
| Full audio, `FZ8BxMU3BYc` | 3,214,890 source bytes; MP3 4,673,309 bytes, 192.072 seconds, stereo 48 kHz, title and attached JPEG. Independent full decode passed. Snapshot: `../artifacts/live/download.json`. |
| Separate audio/video, same ID | MP4 5,417,204 bytes: H.264 256×144, AAC and attached cover. Video 192.025167 s, audio 192.075465 s, both start at zero; full decode passed. Output retained under `/private/tmp/tuneka-visionos-video-output/`. This validates the tested 144p combination, not all quality/container combinations. |
| Playlist continuation | `PLwMEL7UNT4o9iMzrvNBXZqXbNPFfT6rVD`: first and second pages each returned 100 items with continuation; neither page falsely claimed completion. |
| Pause and recovery, `M4gD1WSo5mA` | Native GUI acknowledged pause at 6,291,408/11,664,965 bytes. First GUI resume failed with CDN Unavailable. Explicit CLI retry reused the retained prefix and completed all six WAV chapters. All six fully decode, combined duration 720.724172 s. Snapshots: `../artifacts/gui-paused-job.json`, `live/resume.json`, `live/resume-media-validation.json`. The initial failure was not reproduced by the subsequent fresh range request; its exact HTTP status was not captured. Safe errors now include status codes. |
| Cancel during processing | Native GUI FLAC job acknowledged cancellation after two chapters were committed. Both completed files remain; private source/work was removed, with no `.part` output. `../artifacts/gui-cancelled-job.json`. |
| Mixed batch | FZ plus unavailable `0ThMultL4PY`: `partially_completed`, exit 2, first item completed and second has a separate restricted/unavailable error. After the timestamp fix, fresh M4A is 4,920,548 bytes, AAC stereo 48 kHz, 192.037 seconds, with title/attached JPEG and successful full decode. Accepted evidence: `../artifacts/live/batch-fixed.json`, `batch-fixed-exit-code.txt`, `batch-fixed-m4a-validation.json`; the original `batch.json` records the earlier defective artifact. |

Native GUI QA exercised actual URL → selection → completed MP3/M4A, settings persistence, native folder selection, Ukrainian/English, olive/purple/light themes, long/Unicode input, playlist selection guard, pause and cancel. Continuation QA verified actual 640×480 layout, native zoom/restore, Tab/Shift-Tab navigation, Enter/Space activation and keyboard reveal of a completed file in Finder. Observed defects were fixed: buttons/artifacts missing from keyboard navigation, offscreen focused controls, and stale scroll offset after changing screens. Screenshots and observations are in `../artifacts/gui-qa.md` and `../artifacts/screenshots/`. Formal accessibility semantics remain limited because GPUI exposes only the native window in macOS AX. The disposable QA apps were closed and removed; isolated outputs/state were retained.

Full 720p VP9/Opus WebM and H.264/AAC MP4 also completed and fully decoded, with monotonic per-stream DTS and end-time differences of 10 ms/50.299 ms. See `../artifacts/live-video-matrix.md`. That report transparently records an environment-variable mistake: four task-created CLI QA snapshots initially used the normal Rust state directory. After confirming exact IDs, completed states and task output paths, the coordinator acquired their OS locks and archived only those snapshots under `../artifacts/archived-qa-jobs/`. Normal settings/other jobs were untouched; permanent lock inodes were retained.

## Final media verification

Final output inspection caught an M4A timestamp regression missed by the initial tests: a 192-second Opus/WebM source yielded an AAC file with 9003 frames but a reported duration of 4.021 seconds. The original batch file is diagnostic evidence, not an accepted media artifact. The filter sink can retain `1/1000` timestamps when no audio resampler is needed, whereas the encoder expects `1/48000`. Filtered frame PTS is now explicitly rescaled into the encoder timebase. A new fixture reproduces the failure before the fix (2.581333 s of decoded samples incorrectly reported as 0.074 s) and passes afterward for full and trimmed MP3/M4A/FLAC/WAV. It checks container duration, decoded sample count, packet timeline and monotonic DTS. A fresh download of the same live source now reports **192.037 s** and fully decodes successfully; batch partial status/exit 2 also remain correct.

All ten media tests now pass. Added fixtures contain 16 actual decoded H.264 B-frames, zero/nonzero origins and negative DTS, with audio offset by about 300 ms. MP4/MKV mux and all same/cross-container remux combinations preserve decoded frames, exact audio samples, relative A/V start/end within 2 ms and monotonic available DTS. Nonzero-origin VP8/Opus WebM mux/remux passes equivalent checks. FFmpeg can warn about initially unset Matroska DTS; decoded output timelines remain correct in the tested cases. Subjective lip-sync and every possible source codec are not inferred from these fixtures.

## Packaging and platform gates

| Platform | Build | Runtime/package |
|---|---|---|
| macOS ARM64 | All four executables built; 59 baseline tests plus final TUI regression pass | Native GUI including keyboard/compact-window fixes, CLI, PTY TUI and MCP checked. Final `.app` under `../dist/macos-arm64-debug/` rebuilt and deep/strict signature checked. Not a clean-host or notarized release test. |
| Linux ARM64 container | All four executables built; 59 baseline tests plus final TUI regression pass | Real CLI and GUI MP3 downloads fully decode. Xvfb/Openbox + software Vulkan displays GPUI, handles input/errors, resize and graceful quit. Both `.deb` packages install, run and uninstall in clean Debian 13 containers. Physical GPU/Wayland remain unverified. |
| Linux amd64 (emulated on ARM64) | All four executables built; 59 baseline tests plus final TUI regression pass | Both Debian packages install and uninstall in clean Debian 13; unprivileged CLI and X11/Openbox software-Vulkan GUI input/error/resize/quit pass. Screenshots inspected. No physical x86 or Wayland claim. |
| Windows x64 | Unverified | Unverified |
| macOS x64 | Unverified | Unverified |

Docker storage blocker **resolved**: after host disk exhaustion, reads/writes continued returning `metadata_v2.db`/containerd `meta.db` I/O errors despite newly freed space. The user authorized restarting Docker Desktop after inspection identified active `lago_db_dev` and GitHub MCP containers. The supported restart restored image/storage operations; the existing PostgreSQL container restarted and `pg_isready` confirmed readiness. A disposable container write/read also passed. No volume reset or global prune was used. Only task-generated native targets and individually identified Tuneka build-cache entries were cleaned for capacity. Dockerfile cache mounts and separate runtime stages now avoid shipping build intermediates. Final ARM64 read-back caught Cargo reusing an older TUI test binary when copied source mtimes predated a compilation from another build context. Both Docker build stages now refresh workspace/vendor source mtimes before Cargo; the repeated ARM64 run executes both TUI tests, including the new diagnostic regression. The final ARM64 headless package was installed/run/uninstalled again (`../artifacts/linux-arm64-headless-package-final.log`). Historical failure logs remain under `../artifacts/linux-build*.log`.

Linux GPUI additionally exposed an old transitive `xattr 0.2.3` reference to the Linux `ENOATTR` alias removed by libc 0.2.190. A reviewed vendored compatibility patch uses `ENODATA` only on Linux/Android, retaining original source/tests and both licenses. Other targets are unchanged. See `../vendor/xattr/TUNEKA-PATCH.md`.

Linux evidence: `../artifacts/linux-arm64-gui-build-final.log`, `linux-arm64-media-validation.json`, `linux-gui/final-smoke.log`, `linux-gui-live/` (completed job, full decode and screenshot), `linux-arm64-deb-smoke.log`, `linux-arm64-desktop-deb-smoke.log`. amd64 evidence: `../artifacts/linux-amd64-build.log`, `linux-amd64-headless-final.log`, `linux-amd64-desktop-deb-smoke.log`, `linux-amd64-deb-gui/`. Final ARM64 headless refresh: `../artifacts/linux-arm64-headless-final.log`. Windows/macOS x64 still require suitable SDKs, native libraries and runners. Windows ARM64 remains deferred per plan.

The macOS bundle is ad-hoc signed for local development. Debian packages split headless and GUI executables and depend on distribution FFmpeg libraries. Complete redistribution notices/source obligations, release signing/notarization and remaining platform tests remain release gates. `.github/workflows/rust.yml` configures amd64/arm64 headless validation in Debian 13/FFmpeg 7 containers and a separate GUI/X11 smoke job; it has not run remotely. No release, push or infrastructure deployment was performed.

## Review and remaining coverage

Independent reviews covered shared contracts, extraction, media FFI, files/jobs and MCP. Substantive findings were fixed with regression coverage: durable publication receipts, output symlink confinement, paused cancellation cleanup, safe HTML script extraction, finite-range progress beyond retry count, and retained source cleanup after durable completion.

The later [M2 continuation](m2-validation.md) verifies finite HLS MPEG-TS/fMP4/AAC and real current `n` execution. Successful direct/HLS media URLs did not require those challenges, so challenge-bearing CDN acceptance remains open, together with SABR/DASH, exhaustive player layouts, physical Linux GPU/Wayland, formal accessibility and the remaining Windows/macOS x64 release matrix. Unsupported formats/codecs and authentication requirements return explicit errors. Do not equate local success with completion of every M0–M6 acceptance gate.

Final packaging review found no remaining substantive defect after the FFmpeg-version and Cargo-cache fixes. The automated X11 script checks window presence, event delivery, resize and exit; error text/rendering is established by manual inspection of the retained captures, not by its exit code. Its PASS wording now states this limit.
