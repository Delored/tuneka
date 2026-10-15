# M2 CDN and long SABR follow-up — 2026-10-14

This supersedes the two open CDN gates in [the earlier transport validation](m2-transport-validation.md). Both requested full-file checks now pass. Automatic proof acquisition for ordinary long SABR downloads remains unimplemented; the successful SABR checks below explicitly supplied a server-issued proof obtained by a separate QA tool.

## Genuine signatureCipher: complete download

The earlier 403 probe changed the signature but overlooked `n` inside the nested cipher URL. The genuine MWEB itag 18 response contains both challenges. Its matching plasma player (`22cb88bc`, SHA-256 `a81c7a048e217a0067a64340eea0cddc71e6bacb1c3c30e82d2510ad40031b39`) uses a nonzero empty-string table entry for `n`; extraction had incorrectly required index zero. The new recognizer resolves the exact table entry and `set("n", …)` call-site selectors, and rejects ambiguous wrappers, bindings and guards. A synthetic regression covers this layout.

With the same unexpired response, Safari user agent, Referer and byte range:

| Request | Result |
|---|---|
| Transformed signature, original `n` | HTTP 403, empty body |
| Transformed signature and `n` | HTTP 206, 65,536 MP4 bytes, exact Content-Range |

The signature transforms 112 → 104 characters; `n` transforms 17 → 14. Independent execution of the original player confirms the raw signature. The browser's custom query serializer stores an already encoded value; Rust's URL serializer instead encodes the raw result once. Adding another `encodeURIComponent` would double encode `%3D`; no such change remains. An integration regression checks both nested challenges, single signature encoding and preservation of other query values.

`cipher_smoke` then used the exact captured response/player with `Youtube::resolve_player_format`, the shared production challenge executor, normal `Engine`/`HttpDownloader`, and linked FFmpeg. It downloaded all 12,179,156 source bytes and published a **12,235,065-byte MP4**, H.264 640×360 / 4,604 frames and AAC 44.1 kHz. Video duration is 192.025167 s; audio 192.029025 s. Complete independent decode exits zero with empty stderr. This example deliberately selects the actual MWEB cipher; ordinary automatic extraction still prefers direct streams. Captured response validation rejects direct-URL substitution, missing challenges, ambiguous formats and stale expiry.

Evidence: `artifacts/cdn-followup/result.json`, `report.md`, `cipher-full.log`, `cipher-validation.json`. Signed URLs and actual challenge values stay in private transient files, outside the repository.

## Long SABR: proof requirement isolated and transport verified

An uninterrupted run without proof failed at exactly the earlier pause/resume offset: **2,902,987 / 9,080,902 video bytes**, with explicit stream-protection rejection. This rules out pause alone as the cause. Protocol alignment added a per-session `rn` counter advancing on every POST, including retries, and a truthful `BufferedRange.timeRange` in milliseconds. A second no-proof run still failed at that exact offset. These corrections are retained with wire regressions, but are not presented as the fix for protection rejection.

The public protocol distinguishes a limited initial playback window from accepted proof. Current WEB proofs bind to the video ID. A separate anonymous QA process used the public homepage challenge, Google interpreter, GenerateIT and a video-bound mint to obtain an **89-byte proof** for `FZ8BxMU3BYc`. It used `bgutils-js` 4.0.3 / `jsdom` 26.1.0 outside the product. No account cookies or credentials were used. The integrity response's 43,200-second TTL is not established as the lifetime of the resulting video proof; the Rust QA adapter accepts freshly minted input for at most 15 minutes and checks its exact video/WEB binding on every resolution.

`SabrStream.po_token` now carries optional raw proof bytes in memory to protobuf `StreamerContext` field 2. Empty/oversized values fail; proofs are excluded from Debug/Serde, logs, checkpoints and completion receipts. Default `Youtube` supplies no proof. Explicit rejection still fails without publishing partial media, with an error identifying the proof requirement.

Two full own-Rust transport runs with the valid proof passed:

| Check | Result |
|---|---|
| Uninterrupted | Completed, 12,298,707-byte MP4 |
| Pause / fresh stream resolution / resume | Pause acknowledged at 891,812 bytes; completed, 12,298,707-byte MP4 |

Both outputs have H.264 640×360 / 4,604 frames (192.025167 s), AAC 44.1 kHz / 8,272 frames (192.075465 s), cover and zero start times. Both have monotonic DTS and decode completely with empty stderr. The 50.299 ms end difference matches the independently validated DASH source. The familiar cover-related library `timescale not set` diagnostic does not appear in independent output decoding.

Evidence: `artifacts/m2-sabr-followup-{baseline,aligned,proof,proof-pause-retry}.log`, the two successful roots' `validation.json`, and `artifacts/sabr-followup/proof-validation.json`. Read-back found no proof in generated JSON/log evidence; the deterministic regression also checks actual checkpoint/receipt contents. A first pause attempt failed before job creation on an unsupported player layout, and an initial baseline failed on missing auxiliary `ytInitialData`; neither is counted as a successful run.

Three bounded follow-up watch/player captures using the same reqwest/Safari client exercised `22cb88bc` standard, `21090930` standard and `22cb88bc` standard again; all genuine `n` transforms passed. The exact source of the earlier unsupported preflight was not retained, so its cause remains unclassified. Evidence: `artifacts/cdn-followup/rotation-check.json`. Temporary capture hooks were removed from product source.

References: [pinned SABR implementation and schemas](https://github.com/LuanRT/googlevideo/tree/96d1418984d00ac2797a1dafe4625d1368b5b9aa), [pinned BgUtils](https://github.com/LuanRT/BgUtils/tree/0594e3da2eb4ff18a6d72f7d3c4049674bb25bc9). They are research/QA references, not product runtime dependencies. The product uses Rust, bounded QuickJS and linked FFmpeg, without Node, Python, yt-dlp or FFmpeg subprocesses.

## Final validation and reproducibility

macOS ARM64: **127 headless tests pass, zero failures, one ignored subprocess helper** exercised by the lock test. This includes 53 YouTube, 48 downloader and 11 media tests. Formatting, headless all-target Clippy with `-D warnings`, and CLI/TUI/MCP builds pass. Evidence: `artifacts/m2-followup-tests.log`, `m2-followup-clippy-final.log`, `m2-followup-build.log`. Current executable copies and SHA-256 values are in `artifacts/bin-m2-followup-macos-arm64/`.

Linux ARM64 / Debian 13: the same **127 headless tests pass**, zero failures and one ignored helper; all three headless entry points build in `tuneka-rust:m2-cdn-followup-arm64`. Its disposable runtime container downloaded public `jNQXAC9IVRw` through the ordinary direct path and completed a 755,112-byte MP4 (H.264 320×240, AAC 44.1 kHz, cover, 19.063583 s). Packet timestamps and complete decoding pass. Evidence: `artifacts/m2-followup-linux-arm64.log`, `artifacts/m2-followup-linux-runtime/{job,validation}.json`. The container was removed. Full cipher and long SABR runtime proofs above are macOS; this Linux runtime proof is direct transport. Existing GUI/amd64 packages were not rebuilt in this continuation.

Independent reviewers checked the actual plasma call chain and browser signature serialization, then the proof contract, raw protobuf field, nonpersistence, `rn`, buffered ranges and shared decipher integration. No open actionable finding remains from those reviews; the independent SABR run passed 18/18 tests.

Explicit QA examples use a new isolated output root and stop after 180 seconds or 100 MiB. Caller-supplied captures must be private and fresh; they are not checked into the repository. From `rust/`:

```sh
TUNEKA_CIPHER_RESPONSE=/path/to/private/mweb.json \
TUNEKA_CIPHER_PLAYER=/path/to/private/matching-player.js \
cargo run --locked -p tuneka-cli --example cipher_smoke -- \
  artifacts/new-cipher-check FZ8BxMU3BYc mp4

TUNEKA_SABR_PROOF_FILE=/path/to/private/proof.json \
cargo run --locked -p tuneka-cli --example sabr_proof_smoke -- \
  artifacts/new-sabr-proof-check FZ8BxMU3BYc mp4 pause
```

The proof file contract is `video_id`, `client: "WEB"`, unpadded base64url `po_token`, `created_at` (Unix milliseconds), and `ttl_seconds` from the integrity response. The adapter applies the stricter 15-minute QA age cap. It does not mint tokens, authenticate an account, or claim automatic production support. Do not reuse a video-bound proof for another media ID.

## Scope still open

The signatureCipher CDN gate is closed for the validated current player. Long SABR transport and pause/resume with valid proof are closed for this 192-second VOD. **Automatic native proof acquisition/renewal remains open**, so ordinary long SABR operation cannot be claimed complete. The QA adapter is not a shipping token provider. Real provider MPD delivery remains unobserved; DASH SIDX delivery and offline MPD fixtures retain their earlier evidence. Unknown player layouts, ambiguous language variants and unsupported media continue to fail explicitly. Live streams and DRM are outside release scope.

No push, release, deployment or Python application changes were made.
