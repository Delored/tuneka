# Runtime contracts — M0, 2026-10-14

The core owns serializable domain models and object-safe async ports. Adapters depend on core; entry points compose adapters. The CLI crate exposes the composition library shared by TUI/MCP/GPUI. No transport or graphics dependency enters core.

Jobs belong to the creating process. A persistent OS lock inode protects each job/checkpoint for its entire ownership lifetime, including pause. The worker acknowledges pause only after I/O or processing returns and the durable snapshot is saved. Processing restarts from source after pause; it is not suspended inside FFmpeg. Cancellation takes precedence. Final snapshots, not transient progress, are authoritative.

Each playlist occurrence receives a separate item index. Pagination failures retain discovered items and enumeration_complete=false. Successful known items cannot turn incomplete enumeration into completed. Partial completion is exit 2; failed/cancelled are nonzero. Resume skips completed items and requires explicit retry for failures. Signed URLs are adapter-local and refreshed by stable media/format IDs; never serialized into jobs.

Output names are sanitized components, reserved with persistent lock files and published without overwrite on the same filesystem. Settings use locked read-modify-write and atomic replacement. PID is not ownership evidence. Rust data lives separately from Python data and imports are explicit and preserve source.

Selected toolchain: Rust 1.96.0. FFmpeg binding ffmpeg-next 8.0.0, dynamically linked to locally available FFmpeg 7 (libavcodec 61.19.100, libavformat 61.7.100). GPUI registry 0.2.2, separate optional workspace package. QuickJS via rquickjs 0.14.0 was selected after the spike: each VM has a 32 MiB heap quota, 512 KiB stack and 2-second interrupt deadline. Boa was rejected because it lacks the required hard heap quota. Exact dependencies are captured by Cargo.lock.

Initial read-only architectural review identified races around pause/publication and persistent lock inodes. Tests must include actual competing processes, range mismatch, lost progress, playlist continuation failure, and preservation of completed item outputs.

Build and runtime verification for each platform are distinct. This host is macOS; Windows/Linux execution remains unverified until evidence exists.

The successful public extraction client is VISIONOS 1.02 with fresh anonymous watch visitorData. Android metadata/initial bytes were insufficient: later media ranges returned 403. Complete MP3 and MP4 files were subsequently produced through the Rust pipeline and independently decoded. VisitorData is ephemeral and not serialized; no account cookies are used. HTTP media requests use finite 1 MiB ranges, including the first request. A transient live resume failure was recovered by explicit retry without discarding the verified partial file.
