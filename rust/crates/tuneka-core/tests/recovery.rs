use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tuneka_core::{files, *};

struct Metadata;
#[async_trait]
impl MetadataProvider for Metadata {
    async fn inspect(&self, _: &str) -> Result<MediaInfo> {
        Ok(MediaInfo {
            id: "AAAAAAAAAAA".into(),
            title: "Recovery".into(),
            duration: Some(1.0),
            chapters: vec![],
            has_chapters: false,
            thumbnails: vec![],
            warnings: vec![],
            formats: vec![Format {
                id: "140".into(),
                mime_type: "audio/mp4".into(),
                codecs: vec!["mp4a".into()],
                has_audio: true,
                has_video: false,
                bitrate: 1,
                height: None,
                content_length: Some(4),
            }],
        })
    }
    async fn playlist_page(&self, _: &str, _: Option<&str>) -> Result<PlaylistPage> {
        unreachable!()
    }
    async fn resolve_stream(&self, _: &str, _: &str) -> Result<ResolvedStream> {
        Ok(ResolvedStream {
            sabr: None,
            segmented: None,
            media_id: "AAAAAAAAAAA".into(),
            format: self.inspect("").await?.formats.remove(0),
            url: "https://rr1.googlevideo.com/x".into(),
        })
    }
}
struct Download {
    pause: bool,
}
#[async_trait]
impl Downloader for Download {
    async fn download(
        &self,
        _: ResolvedStream,
        path: PathBuf,
        c: Control,
        _: ProgressSink,
    ) -> Result<PathBuf> {
        if self.pause {
            std::fs::write(path.with_extension("part"), b"part")?;
            c.pause();
            c.check()?;
        }
        std::fs::write(&path, b"data")?;
        Ok(path)
    }
}
struct InterruptedPublication {
    calls: AtomicUsize,
}
#[async_trait]
impl MediaProcessor for InterruptedPublication {
    async fn process(&self, r: ProcessRequest, _: Control) -> Result<(Artifact, Vec<String>)> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let data = b"published result";
        files::atomic_json(
            &r.publication_receipt,
            &serde_json::json!({"path":r.output,"size":data.len(),"sha256":format!("{:x}",Sha256::digest(data))}),
        )?;
        std::fs::write(&r.output, data)?;
        // Emulate abrupt loss of the result before the scheduler persists its artifact.
        Err(Error::new(
            ErrorCode::Paused,
            "publication returned no persisted artifact",
        ))
    }
}
fn request(root: &Path) -> DownloadRequest {
    DownloadRequest {
        urls: vec!["https://youtu.be/AAAAAAAAAAA".into()],
        output_dir: root.join("out"),
        format: OutputFormat::Flac,
        max_height: None,
        chapters: vec![],
        playlist_items: vec![],
    }
}
fn engine(root: &Path, media: Arc<InterruptedPublication>, pause: bool) -> Arc<Engine> {
    Engine::new(
        Arc::new(Metadata),
        Arc::new(Download { pause }),
        media,
        root.join("state"),
    )
    .unwrap()
}

#[tokio::test]
async fn publication_before_snapshot_is_recovered_across_engine_restart() {
    let d = tempfile::tempdir().unwrap();
    let media = Arc::new(InterruptedPublication {
        calls: AtomicUsize::new(0),
    });
    let first = engine(d.path(), media.clone(), false);
    let id = first.create(request(d.path())).unwrap();
    let paused = first.wait(&id).await.unwrap();
    assert_eq!(paused.state, JobState::Paused);
    assert!(paused.items[0].artifacts.is_empty());
    assert!(d.path().join("out/Recovery.flac").exists());
    drop(first);
    // A killed worker leaves the last durable Processing snapshot, with no
    // returned artifact. Restore that exact state while retaining committed media.
    let mut crash_snapshot = paused;
    crash_snapshot.state = JobState::Processing;
    crash_snapshot.items[0].state = JobState::Processing;
    files::atomic_json(
        &d.path().join("state/jobs").join(format!("{id}.json")),
        &crash_snapshot,
    )
    .unwrap();
    // Give the finished worker's Arc a chance to drop its exclusive job lock.
    tokio::task::yield_now().await;
    let resumed = engine(d.path(), media.clone(), false);
    resumed.restore(&id, false).unwrap();
    let done = resumed.wait(&id).await.unwrap();
    assert_eq!(done.state, JobState::Completed);
    assert_eq!(done.items[0].artifacts.len(), 1);
    assert_eq!(media.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read(&done.items[0].artifacts[0].path).unwrap(),
        b"published result"
    );
}

#[tokio::test]
async fn damaged_published_file_is_never_adopted_or_overwritten() {
    let d = tempfile::tempdir().unwrap();
    let media = Arc::new(InterruptedPublication {
        calls: AtomicUsize::new(0),
    });
    let e = engine(d.path(), media.clone(), false);
    let id = e.create(request(d.path())).unwrap();
    assert_eq!(e.wait(&id).await.unwrap().state, JobState::Paused);
    let target = d.path().join("out/Recovery.flac");
    let replacement = b"different result"; // same length: digest, not size, must detect it.
    std::fs::write(&target, replacement).unwrap();
    e.resume(&id, false).unwrap();
    let failed = e.wait(&id).await.unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(
        failed.items[0].error.as_ref().unwrap().code,
        ErrorCode::Conflict
    );
    assert_eq!(std::fs::read(target).unwrap(), replacement);
    assert_eq!(media.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancel_paused_job_removes_durable_partial_work() {
    let d = tempfile::tempdir().unwrap();
    let media = Arc::new(InterruptedPublication {
        calls: AtomicUsize::new(0),
    });
    let e = engine(d.path(), media, true);
    let id = e.create(request(d.path())).unwrap();
    assert_eq!(e.wait(&id).await.unwrap().state, JobState::Paused);
    let work = d.path().join("state/work").join(&id);
    assert!(work.exists());
    e.cancel(&id).unwrap();
    assert_eq!(e.get(&id).unwrap().state, JobState::Cancelled);
    assert!(!work.exists());
}

#[test]
fn corrupt_and_wrong_path_publication_receipts_fail_closed() {
    let d = tempfile::tempdir().unwrap();
    let output = d.path().join("output");
    std::fs::write(&output, b"safe").unwrap();
    let receipt = d.path().join("receipt.json");
    std::fs::write(&receipt, b"{broken").unwrap();
    assert_eq!(
        files::recover_publication(&receipt, &output)
            .unwrap_err()
            .code,
        ErrorCode::Protocol
    );
    files::atomic_json(&receipt,&serde_json::json!({"path":d.path().join("other"),"size":4,"sha256":format!("{:x}",Sha256::digest(b"safe"))})).unwrap();
    assert_eq!(
        files::recover_publication(&receipt, &output)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(std::fs::read(output).unwrap(), b"safe");
}
