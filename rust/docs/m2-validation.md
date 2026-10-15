# M2 continuation — 2026-10-14

This is the earlier HLS validation snapshot. The subsequent DASH/SABR and genuine challenge work is recorded in [m2-transport-validation.md](m2-transport-validation.md), which supersedes the open transport gates and test counts below.

The own Rust extractor/downloader now supports clear finite YouTube HLS VOD, and the current indexed player challenge layout. This report separates complete media downloads from challenge execution alone. Historical M0–M6 platform packages in `validation.md` predate this M2 continuation unless explicitly stated otherwise.

## Live transport checks

All cases used public anonymous `FZ8BxMU3BYc`, isolated job/output directories and bounded opt-in checks. Signed URLs, cookies and challenge values are not persisted.

| Source / output | Actual result | Evidence under `../artifacts/` |
|---|---|---|
| HLS AVC `hls-230`, 37 MPEG-TS segments + AAC `hls-234`, 37 packed-AAC segments → MP4 | Completed; 12,346,281 bytes; H.264 640×360 + AAC 44.1 kHz and cover; video 192.025156 s, audio 192.029025 s, start offset 41 ms. Full independent decode succeeds with no errors. | `m2-hls-live-ts.log`, `m2-hls-live-ts/validation.json` |
| HLS VP9 `hls-603`, one init + 37 fMP4 media segments + separate AAC → MKV | Acknowledged pause at 161,898 bytes, refreshed signed URLs, resumed and completed. 5,049,166 bytes; VP9 256×144 + AAC 44.1 kHz; both start at zero; end difference 5 ms. All 4,604 video frames decode with strictly increasing timestamps. | `m2-hls-resume-final.log`, `m2-hls-resume-final/validation.json` |
| Two independently resolved VP9 plans | Identical stable identity and segment count; initialization, first and last media segments are byte-identical. Only reported URL field names changed. | `m2-hls-refresh-final.log` |
| Default direct transport → MP3 after integration | Completed; 4,673,309 bytes; 192.072 s, stereo 48 kHz with cover. Full independent decode succeeds with empty stderr. | `m2-direct-final.json`, `m2-direct-final/validation.json` |

The first default `ffmpeg -f null` MKV validation rounded variable-frame-rate timestamps to duplicate ticks and warned from the output null muxer. Repeating the full decode with `-fps_mode passthrough -enc_time_base:v demux` succeeds with empty stderr; input packets and decoded frame timestamps are independently checked. Both diagnostic and corrected validation results are retained. MKV cover embedding returns its explicit unsupported-cover warning.

The discovery response advertises HLS but no DASH manifest. `m2-discovery/report.md` records master/audio groups, MPEG-TS, fMP4 initialization and packed AAC. No DASH/live/encrypted transport success is inferred.

## Problems fixed

- HLS-only metadata previously failed before resolution; metadata now survives to manifest extraction. Automatic selection falls back when direct formats have no transport URL. The opt-in test can force HLS on a source that offers both.
- Manifest parsing now handles quoted attributes, external audio groups, repeated video variants, initialization segments and explicit/implicit ranges. Missing audio groups, conflicting duplicate codecs, oversized repeated rows and changed init identities have regression coverage.
- Resume initially rejected the same live resource because `bui`, `cps`, `met` and `epbp` request-routing fields changed. Byte comparisons established unchanged resources. Stable identity now excludes these known transient fields while retaining media/format/codecs, timeline, sequence, ranges, initialization identity and unknown resource selectors. Changed resources still fail closed.
- Segmented download checkpoints contain only identity and committed boundaries, never URLs. Pause/retry truncates uncommitted tails; expiry requests a fresh resolver plan. Range/status/length checks, bounded retries, deadlines, cancellation, cross-process locks and durable publication receipts also apply to segmented input.
- Metadata, manifest and player-script reads now retry transient failures up to three times, with 10-second request limits and bounded backoff. Access denials do not retry. A live preflight timeout prompted this fix; offline HTTP regressions cover server errors, truncated bodies, retry exhaustion and sanitized failures.
- The FFmpeg library input allowlist now accepts local MPEG-TS. Manifests never reach FFmpeg and network protocols remain disabled. A library-generated MPEG-TS/B-frame regression verifies actual demux/remux/decode.
- Full parallel tests exposed a real lock-lifetime race: a concurrently spawned child can briefly retain the open-file description after the parent closes it, causing immediate resume to report a false conflict. `FileLock` now explicitly unlocks when its guard drops. Both a deterministic duplicated-description regression and real subprocess stress fail before the fix and pass afterward; cross-process mutual exclusion still passes.

Initial identity conflicts and the transient preflight timeout remain in `m2-hls-live-fmp4*.log`; those runs are diagnostic failures, not accepted artifacts. Successful replacement evidence is explicitly named above.

## Current player challenges

Public player revision `22cb88bc` uses indexed string tables and flattened numeric dispatcher branches, which the previous extractor did not recognize. The bounded extractor now isolates the relevant definitions, resolves the selected dispatcher branch and required guard constants, and rejects the current obfuscated failure prefix. QuickJS has no host filesystem/network API and retains its memory, stack and execution limits. The Rust extraction phase is also bounded.

Two actual current WEB responses (`FZ8BxMU3BYc`, `M4gD1WSo5mA`) supplied genuine `n` inputs on SABR URLs. Standard and TCC player variants transform those inputs from 17 to 14 characters. The actual downloaded players also execute synthetic 62-character signature inputs to 54-character results. Sanitized evidence: `../artifacts/challenge-live/report.md` and `result.json`. The minimized public-code regression fixture retains its original notices and adjacent license.

**This proves current challenge execution, not CDN acceptance for a challenge-bearing download.** The bounded public-client sweep did not return a genuine `signatureCipher`; current WEB delivery uses unsupported SABR. No URL mode was rewritten or authentication bypass attempted. The accepted HLS/direct downloads do not prove signature challenge handling at the CDN.

## M2 status and remaining limits

Final macOS ARM64 checks: **96 tests passed, zero failures, one ignored subprocess helper** (exercised through the lock tests); workspace formatting, all-target Clippy with `-D warnings`, and all four executable builds pass. This includes 36 YouTube tests (16 player tests), 30 downloader tests and 11 media tests. Logs: `../artifacts/m2-tests-workspace-final.log`, `m2-clippy-workspace-final.log`, `m2-build-workspace-final.log`. The original failed integration/lock-stress runs remain as diagnostic evidence. Existing upstream `block` future-compatibility and vendored `xattr` build warnings are separate from project Clippy success.

Final Linux ARM64/Debian 13 checks: **92 headless tests passed, zero failures, one ignored subprocess helper**; CLI/TUI/MCP built into local image `tuneka-rust:m2-arm64`. A disposable container then completed a real default-transport MP3 download (4,673,309 bytes, 192.072 s, 48 kHz with cover), independently decoded with empty stderr. Evidence: `../artifacts/m2-linux-arm64-build.log`, `m2-linux-runtime/job.json`, `m2-linux-runtime/validation.json`. The container exited and was removed. This is current Linux headless build/runtime evidence; M2 was not repackaged into the older Debian/macOS bundles or rerun on amd64/GUI platforms.

The final native TUI PTY smoke also passed (`../artifacts/m2-tui-pty.log`). Current macOS executables and SHA-256 hashes are retained in `../artifacts/bin-m2-macos-arm64/`; they use host libraries and are not distribution bundles. After disk space fell below 1 GiB during verification, only this task's native Cargo target and the exact newly created Tuneka ARM64 target-cache entry were removed. Source, evidence, completed media, executable copies, the built Linux image, unrelated Docker caches and all volumes remain intact.

The locally available M2 work covers metadata, chapters/thumbnails, explicit playlist pagination, direct and finite HLS separate streams, output validation, URL refresh, bounded retries, pause/cancel and durable resume. Offline tests remain separate from opt-in live examples. Independent reviewers checked HLS semantics, downloader lifecycle and challenge bounds.

Open compatibility gates are genuine signatureCipher-to-CDN/media acceptance, `n`-bearing CDN/media acceptance, SABR, DASH and additional future player layouts. Live/upcoming, encrypted/DRM and discontinuous HLS remain explicit unsupported cases. M2 is therefore locally functional with a substantially expanded live matrix, not a claim of universal YouTube transport support. No push, release or infrastructure deployment is part of this work.
