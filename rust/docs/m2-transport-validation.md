# M2 DASH, SABR and genuine challenges — 2026-10-14

**Historical snapshot:** [the subsequent CDN follow-up](m2-cdn-followup.md) corrects the signature-only 403 probe and verifies full genuine cipher delivery plus long SABR with a valid proof. Counts and open gates below describe the earlier run.

This continuation supersedes the transport gates in [m2-validation.md](m2-validation.md). Tests use public anonymous VOD, isolated output directories and bounded opt-in checks. Signed URLs, challenge values and opaque session data are excluded from durable evidence. Product runtime uses Rust, bounded QuickJS and linked FFmpeg libraries; Python/FFmpeg command-line tools are independent diagnostics only.

## DASH

Implemented clear static single-period MP4 addressing: SegmentList, finite SegmentTemplate/SegmentTimeline, and version 0/1 flat SIDX. YouTube player JSON `initRange`/`indexRange` is also supported directly. No synthetic MPD is manufactured. XML size/depth/node counts, repetitions, segment count, URL hosts and byte ranges are bounded. Dynamic/multiple-period/encrypted manifests, negative repeat counts, hierarchical SIDX and unsupported inheritance fail explicitly.

The actual VISIONOS response for `FZ8BxMU3BYc` supplied indexed on-demand URLs, not an MPD. The accepted live check selected AVC itag 134 (360p) and AAC 140. It acknowledged pause at 663,391 bytes, resolved fresh URLs, resumed, muxed with linked FFmpeg and completed. Independent validation found:

- MP4: 12,298,707 bytes; H.264 640×360, 4,604 frames, AAC 44.1 kHz and cover.
- Video 192.025167 s; audio 192.075465 s; both start at zero; end difference 50.299 ms.
- Monotonic packet DTS; complete decode exits zero with empty stderr.

Evidence: `artifacts/m2-dash-live-fixed.log`, `artifacts/m2-dash-live-fixed/validation.json`. Failed initial identity checks remain diagnostic history. Real MPD delivery is not claimed; MPD forms have deterministic fixture coverage.

The same indexed media refreshes with a rotating opaque `id=o-…` query field. Its exclusion is restricted to the hash-only identity copy when a valid authoritative video ID, matching itag, last-modified value, declared content length and exact SIDX hash bind the resource. Other IDs/selectors remain identity-bearing. Actual download URLs are unchanged. Regressions reject changed resource selectors, content identity, SIDX and malformed SegmentList entries. Exact HTTP 206/Content-Range/length is required, truncated reads retry, and 401/403/410 report unavailable.

## SABR

The own downloader implements bounded protobuf/UMP response parsing and finite clear MP4 audio/video sessions. Only complete media segments advance durable checkpoints. Selected bytes are written contiguously; peer segments are consumed and reported truthfully. Sequence, format identity, byte offset, length and SHA-256 protect resume. Saved prefixes are read back before continuation; partial response tails never publish. Cookie/context updates remain in RAM, in wire order; start/stop/discard state survives correctly within the session. Cancellation, pause, retries, deadlines, redirects and no-progress limits are bounded. Explicit server protection rejection/errors fail without publishing a partial file.

Initial genuine `n` validation used the original WEB SABR URL, changing only the current player challenge result. Two bounded POST responses returned HTTP 200 `application/vnd.yt-ump` and complete init/media segments, with continuation reaching audio 49.923 s and video 39.873 s. No account cookies or proof token were supplied. Evidence: `artifacts/sabr-research/probe-result.json`, `probe-continuation-result.json`. These initial probes prove challenge-bearing CDN media delivery, not an entire output file.

A complete product run on public `jNQXAC9IVRw` then passed: AVC itag 133 + standard AAC 140, acknowledged pause at 433,081 bytes, fresh URL resolution and resume, then linked-library mux to `Me at the zoo.mp4` (760,072 bytes). Independent validation confirms H.264 320×240 (284 frames, 18.933333 s), AAC 44.1 kHz (19.063583 s), both starting at zero, monotonic DTS, cover and full decode with empty stderr. Evidence: `artifacts/m2-sabr-short-fixed.log`, `artifacts/m2-sabr-short-fixed/validation.json`. The library emitted a cover-related `timescale not set` diagnostic during processing; the finished media passes independent packet/decode checks.

The 192-second `FZ8BxMU3BYc` run acknowledged pause/resume but later received explicit SABR protection rejection. It stopped with `Unavailable` after 2,902,987 of 9,080,902 selected video bytes. Independent read-back confirmed 13 complete contiguous segments, matching SHA-256 for every segment and exact `.part` size, with no output, target or completion receipt. This remains an open anonymous-delivery compatibility gate, not a complete download. Evidence: `artifacts/m2-sabr-live-diagnostic.log` and its isolated state directory. Pending protection is tolerated only while real media progress occurs; explicit rejection is never ignored.

The short check also exposed duplicate itag 140 variants: standard audio and dynamic-range-compressed (DRC) audio with different revision/xtags. SABR now selects the standard variant and binds resolution to that exact filtered metadata, even when the DRC row appears first. Muxed/progressive variants are excluded from this separate-track transport. Remaining ambiguous same-itag variants fail explicitly; multilingual variant selection is not implemented.

Protocol reference: [LuanRT/googlevideo pinned schemas](https://github.com/LuanRT/googlevideo/tree/96d1418984d00ac2797a1dafe4625d1368b5b9aa), MIT; notice and license retained. It is not a runtime dependency.

Refreshing a stream now parses only the player response and HTML, so missing auxiliary `ytInitialData` no longer prevents an otherwise valid URL refresh. Initial inspection still requires its complete metadata.

## Current n layouts

Actual reqwest watch requests alternated between player revisions `22cb88bc` and `21090930`, including standard, TCE and TCC variants. The latter uses a direct string-table split and AND-mask dispatcher branch instead of the earlier XOR-split/OR-mask shape. Its actual indexed `set("n", ...)` wrapper establishes selector `of(9,2077,input)`. The extractor validates the wrapper, unique definitions, parameter/XOR bindings, table values and guard constant. Independent review caught and fixed ambiguous-wrapper and binding checks. A tiny synthetic regression covers extraction and rejection; full captured-player execution and the live media checks are distinct evidence. Unknown layouts remain unsupported.

## Genuine signatureCipher

A fresh anonymous MWEB response supplied a genuine progressive itag 18 `signatureCipher`, with a 112-character signature input. Its matching mobile plasma player revision `22cb88bc` exposed a previously unsupported direct split/helper/join dispatcher branch. The extractor now identifies its exact call-site selector, validates the branch, retains required definitions and executes it under existing QuickJS limits. The genuine input transformed to 104 characters in about 150 ms. A minimized licensed fixture and selector-mismatch regression cover the layout.

The exact original cipher URL plus transformed signature returned HTTP 403 with an empty body for a bounded CDN range request. Signature execution is confirmed; signature-to-CDN acceptance remains unproven. Current MWEB client policy may require additional delivery authorization, but the 403 alone does not establish its cause. No authentication or URL-mode bypass was attempted. Evidence: `artifacts/challenge-cdn/report.md`, `result.json`.

## Validation status

Final macOS ARM64 checks: **122 headless tests passed, zero failures, one ignored subprocess helper** (exercised by lock tests). This includes 51 YouTube tests, 45 downloader tests and 11 media tests. `cargo fmt --all -- --check`, headless all-target Clippy with `-D warnings`, and CLI/TUI/MCP builds pass. Evidence: `artifacts/m2-transports-tests-final.log`, `m2-transports-clippy-final.log`, `m2-transports-build-final.log`; current executables and hashes are in `artifacts/bin-m2-transports-macos-arm64/`.

Final Linux ARM64/Debian 13 checks: **the same 122 headless tests passed**, zero failures and one ignored helper; CLI/TUI/MCP built into local image `tuneka-rust:m2-transports-arm64`. A disposable container then downloaded public `jNQXAC9IVRw` through the default direct transport and completed a 755,112-byte MP4: H.264 320×240, AAC 44.1 kHz, cover, 19.063583 s. Independent packet/full-decode checks pass with empty decode stderr. Evidence: `artifacts/m2-transports-linux-arm64.log`, `artifacts/m2-transports-linux-runtime/job.json`, `validation.json`. The container was automatically removed. Linux runtime proof is direct transport; full SABR and DASH runtime evidence above is macOS. Existing upstream `block` future-compatibility and vendored `xattr` warnings are not project Clippy errors.

The initial final-suite/native and Linux attempts caught the direct-signature recognizer intercepting the already-supported plasma fixture before shape gating. The gate now runs before decoder lookup; the entire native and Linux suites were repeated successfully. Diagnostic failures remain in the `*-initial.log` files. The stored genuine plasma transform was also rerun successfully after this correction. Independent reviewers covered DASH addressing, SABR framing/checkpoints/context ordering, provider variant selection and the current n-selector call chain.

Earlier macOS/amd64/GUI bundles in `docs/validation.md` predate this continuation; no distribution repackaging or new GUI/amd64 runtime pass is claimed. SABR protocol attribution/license is retained and packaging scripts include the new license directory; their syntax was checked, but package execution was not repeated.

M2's accessible transport work is complete for the validated subset. Two acceptance gates remain: genuine signatureCipher CDN delivery (HTTP 403), and full anonymous long-video SABR delivery (explicit server protection rejection). Actual provider MPD delivery remains unobserved; only real on-demand SIDX and offline MPD cases are validated. Unsupported future player layouts and ambiguous language variants fail explicitly. Live/upcoming content and DRM remain outside release scope. No publication, push or deployment was performed.
