# Tuneka Rust

Independent native implementation under `rust/`. The existing Python/Tkinter application is untouched. The workspace shares one scheduler/extractor/downloader/media pipeline across CLI, Ratatui, MCP stdio and GPUI. A separate root Rust CI workflow is the implementation plan's explicit exception to the subtree boundary.

## Build

Rust 1.96.0, pkg-config, clang and FFmpeg development libraries are required. The tested host uses FFmpeg 7 (`libavcodec 61`, `libavformat 61`). Product binaries dynamically link FFmpeg and never run `ffmpeg`, `ffprobe`, Python or yt-dlp. FFmpeg CLI can be used independently to validate outputs.

```sh
cd rust
cargo build --locked -p tuneka-cli -p tuneka-tui -p tuneka-mcp
cargo build --locked -p tuneka-gui
sh packaging/check.sh
cargo test -p tuneka-gui
```

On macOS install FFmpeg development libraries through your existing package manager. GPUI uses its supported `runtime_shaders` feature, so an absent offline Metal compiler does not require modifying Xcode. Actual window/input/runtime coverage is recorded in `docs/validation.md`. CLI/TUI/MCP default members do not compile GPUI.

```sh
cargo run -p tuneka-cli -- inspect 'https://youtu.be/FZ8BxMU3BYc' --json
cargo run -p tuneka-cli -- playlist 'https://www.youtube.com/playlist?list=PLwMEL7UNT4o9iMzrvNBXZqXbNPFfT6rVD'
cargo run -p tuneka-cli -- download 'https://youtu.be/FZ8BxMU3BYc' --audio mp3 --output ./downloads --json
cargo run -p tuneka-cli -- download 'https://youtu.be/aqz-KE-bpKQ' --video --container mp4 --quality 1080p --output ./downloads
cargo run -p tuneka-cli -- resume JOB_ID
cargo run -p tuneka-cli -- resume JOB_ID --retry-failed
cargo run -p tuneka-cli -- cancel JOB_ID
cargo run -p tuneka-tui
cargo run -p tuneka-gui
TUNEKA_DOWNLOADS=/absolute/allowed/output cargo run -p tuneka-mcp
```

Multiple download URLs are positional arguments. `--chapters 1,3` and `--items 1,5` use one-based indices. Empty selection means all; audio with actual chapters is split into numbered tracks in a title folder. A video without chapters yields one track directly in the output directory. Existing completed files are never overwritten. Unsupported codec/container combinations fail explicitly; expensive video transcodes are not implicit.

Download/resume stdout is an authoritative JSON job snapshot, including per-item state, artifacts, warnings and errors. Diagnostics and job IDs go to stderr. Exit codes: 0 complete, 2 partial, 1 failure/input error, 75 paused, 130 cancelled; clap usage errors use 2. Ctrl-C requests a durable pause, then waits for worker acknowledgement. `cancel JOB_ID` cancels an inactive persisted job without starting its worker. `--retry-failed` explicitly retries failed/cancelled work while preserving completed items.

## Jobs and files

Each process owns its active jobs; there is no daemon. Exiting TUI/GUI or disconnecting MCP pauses/checkpoints work before shutdown. Processing restarts from the downloaded source after pause. Media publication has a durable SHA-256 receipt so a crash before snapshot persistence can be recovered safely. Downloads validate range offsets, stream identity and durable byte count. Signed URLs are refreshed, never saved in JSON or logs. Failed partial downloads remain resumable; cancellation clears private work but retains published artifacts.

Persistent `.lock` files reserve output names and job/checkpoint ownership through OS file locks. Their presence alone does not mean a job is locked; they are deliberately not unlinked. Conflict is controlled across processes. Settings use a separate locked read-modify-write.

Rust settings/checkpoints are separate from Python: `~/Library/Application Support/TunekaRust` on macOS, `%APPDATA%/TunekaRust` on Windows, `$XDG_DATA_HOME/tuneka-rust` (or `~/.local/share/tuneka-rust`) on Linux. `TUNEKA_RUST_DATA_DIR` overrides state; `TUNEKA_DOWNLOADS` overrides the default output. Explicit `import-settings PATH` imports compatible fields once and preserves the original JSON.

## MCP

The rmcp stdio server exposes inspect_media, list_formats, list_playlist_items, create_download, get_job, list_jobs, pause_job, resume_job, cancel_job. Create returns job_id immediately; use get_job until terminal. `partially_completed` requires inspecting per-item results and enumeration_complete. Playlist listing is explicitly paginated. MCP outputs are restricted to its configured output root, including resumed jobs. No arbitrary command/FFmpeg arguments are accepted. Stdout is protocol-only.

## Validation and limits

Offline tests use local HTTP servers and real library-generated media fixtures. Live checks are opt-in:

```sh
cargo run -p tuneka-youtube --example live_smoke -- 'https://youtu.be/FZ8BxMU3BYc' --probe
cargo run -p tuneka-cli --example hls_smoke -- /absolute/new/isolated-hls-check FZ8BxMU3BYc mkv pause
cargo run -p tuneka-cli --example dash_smoke -- /absolute/new/isolated-dash-check FZ8BxMU3BYc mp4 pause
cargo run -p tuneka-cli --example sabr_smoke -- /absolute/new/isolated-sabr-check FZ8BxMU3BYc mp4 pause
```

The extractor prefers direct streams and falls back to available clear HLS, DASH or WEB SABR delivery. Finite HLS VOD supports MPEG-TS, fMP4 and packed AAC. DASH supports static single-period MP4 SegmentList/finite SegmentTemplate and flat SIDX, including real player JSON index ranges. SABR uses bounded protobuf/UMP sessions and durable, identity-checked completed segments. Explicit examples select each transport for validation even when direct streams exist.

Encrypted/live/discontinuous HLS, dynamic/multiple-period DASH and unsupported SABR responses fail explicitly. Current indexed/mobile plasma challenges run in bounded QuickJS. Full direct, HLS, DASH, short SABR and genuine MWEB `signatureCipher` downloads are verified. Both signature and nested `n` must be transformed; the earlier signature-only probe caused its 403. A 192-second SABR download also passed, including pause/resume, with a server-issued video-bound proof supplied by an external QA tool. The downloader forwards optional proof bytes in memory; automatic native proof acquisition/renewal is not implemented. Default long SABR can still report that a valid proof is required. DRC and ambiguous language variants are not selected. See `docs/m2-cdn-followup.md` and `docs/m2-transport-validation.md` for evidence and limits.

Read `docs/validation.md` and `IMPLEMENTATION_PLAN.md` for actual pass/fail evidence and open limits; source/build existence is not release completion. `.github/workflows/rust.yml` configures Linux amd64/arm64 headless gates and a separate Linux GUI/X11 smoke job. The **Rust artifacts** workflow (`.github/workflows/rust-artifacts.yml`) builds downloadable release-profile packages for Linux amd64/arm64, macOS Intel/Apple Silicon and Windows x64. See [artifact names, remote run evidence and manual launch instructions](packaging/README.md). The first artifact run passed both Linux jobs; local Windows/macOS packaging fixes still need remote verification. Existing Python CI is unchanged.

Local macOS development bundle: `python3 packaging/bundle_macos.py --profile debug` after building all four executables. It copies and rewrites the native dependency closure and ad-hoc signs locally. Linux Debian 13 packages and reproducible container checks are described in `packaging/README.md`. GPUI's old xattr dependency has a small licensed Linux compatibility patch under `vendor/xattr`. Windows/macOS x64, signing/notarization and complete release redistribution compliance remain separate gates. See `THIRD_PARTY_NOTICES.md`.
