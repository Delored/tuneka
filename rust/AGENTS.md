# Rust Implementation Guidelines

Read `IMPLEMENTATION_PLAN.md` before implementation. Its requirements are authorized by the user. This subtree is the Rust version; the root guide's Python/Tkinter runtime preference applies to the existing Python application, not to `rust/`.

- Implement a Cargo workspace here with shared core logic for CLI, TUI, MCP and GPUI. Preserve the Python application and its tests.
- Use `rusty_ytdl` only as a reference, not a runtime dependency. Implement the required YouTube subset in Rust; an embedded JavaScript engine is allowed. Preserve attribution when reusing licensed source.
- Link FFmpeg libraries through a dedicated media module. Do not invoke Python, yt-dlp, ffmpeg or ffprobe as product runtime dependencies.
- Follow idiomatic Rust, rustfmt and Clippy. Pin reviewed dependency versions and commit-ready Cargo.lock; keep generated files and credentials out of source control.
- Separate deterministic offline tests from explicit live YouTube checks. Validate real media output, cancellation, partial playlists and resumable downloads. Record platform checks as build or runtime evidence accurately.
- The user explicitly authorizes subagents and model selection for architecture, implementation, review and testing. Follow the role/model table in the plan, pass narrow context, use disjoint file ownership and keep concurrency modest.
- The coordinator owns shared contracts, workspace manifests and lockfile. Reviewers are read-only unless assigned a specific fix. Fix substantive review findings and verify integration before marking a milestone complete.
- Maintain milestone status in the plan. Do not stop at scaffolding or mock demonstrations; continue the authorized implementation while meaningful progress is possible.
- Publishing releases, deploying infrastructure and pushing changes are outside the current task. Prepare and validate local artifacts without performing those actions.
