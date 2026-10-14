use async_trait::async_trait;
use std::{path::PathBuf, sync::Arc};
use tuneka_core::{files::*, *};
#[test]
fn url_and_safe_names() {
    assert_eq!(
        parse_url("https://youtu.be/FZ8BxMU3BYc").unwrap(),
        YoutubeUrl::Video("FZ8BxMU3BYc".into())
    );
    for url in [
        "http://youtube.com/watch?v=FZ8BxMU3BYc",
        "https://youtube.com.evil.test/watch?v=FZ8BxMU3BYc",
        "https://user@youtube.com/watch?v=FZ8BxMU3BYc",
        "https://youtube.com:123/watch?v=FZ8BxMU3BYc",
    ] {
        assert!(parse_url(url).is_err());
    }
    assert_eq!(safe_name("../../NUL", false), "....NUL");
    assert_eq!(safe_name("CON.txt", false), "_CON.txt");
    assert_eq!(safe_name("..", false), "track");
    assert!(!safe_name("a/b\\c", false).contains('/'));
}
#[test]
fn chapters_fill_unknown_end_and_fallback() {
    let c = normalize_chapters("one", None, vec![]).unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].end, None);
    let c = normalize_chapters(
        "x",
        Some(4.0),
        vec![
            Chapter {
                title: "a".into(),
                start: 0.0,
                end: None,
            },
            Chapter {
                title: "b".into(),
                start: 1.5,
                end: None,
            },
        ],
    )
    .unwrap();
    assert_eq!(c[0].end, Some(1.5));
    assert_eq!(c[1].end, Some(4.0));
    assert!(
        normalize_chapters(
            "x",
            None,
            vec![Chapter {
                title: "x".into(),
                start: f64::NAN,
                end: None
            }]
        )
        .is_err()
    );
}
#[test]
fn persistent_lock_and_no_overwrite() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("file");
    let r = Reservation::acquire(path.clone()).unwrap();
    assert!(Reservation::acquire(path.clone()).is_err());
    drop(r);
    std::fs::write(&path, b"old").unwrap();
    assert!(Reservation::acquire(path.clone()).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"old");
}
struct Metadata;
#[async_trait]
impl MetadataProvider for Metadata {
    async fn inspect(&self, url: &str) -> Result<MediaInfo> {
        if url.contains("BBBBBBBBBBB") {
            return Err(Error::new(ErrorCode::Unavailable, "removed"));
        }
        Ok(MediaInfo {
            id: "AAAAAAAAAAA".into(),
            title: "A".into(),
            duration: Some(1.0),
            chapters: vec![],
            has_chapters: false,
            thumbnails: vec![],
            formats: vec![Format {
                id: "1".into(),
                mime_type: "audio/mp4".into(),
                codecs: vec!["mp4a".into()],
                has_audio: true,
                has_video: false,
                bitrate: 1,
                height: None,
                content_length: Some(4),
            }],
            warnings: vec![],
        })
    }
    async fn playlist_page(&self, _: &str, c: Option<&str>) -> Result<PlaylistPage> {
        if c.is_some() {
            return Err(Error::new(ErrorCode::Network, "page failed"));
        }
        Ok(PlaylistPage {
            title: "P".into(),
            items: vec![PlaylistItem {
                id: "AAAAAAAAAAA".into(),
                title: "A".into(),
                available: true,
            }],
            continuation: Some("next".into()),
            complete: false,
        })
    }
    async fn resolve_stream(&self, id: &str, _: &str) -> Result<ResolvedStream> {
        Ok(ResolvedStream {
            sabr: None,
            segmented: None,
            media_id: id.into(),
            format: self.inspect(id).await?.formats.remove(0),
            url: "https://example.googlevideo.com/".into(),
        })
    }
}
struct Download;
#[async_trait]
impl Downloader for Download {
    async fn download(
        &self,
        _: ResolvedStream,
        path: PathBuf,
        control: Control,
        _: ProgressSink,
    ) -> Result<PathBuf> {
        control.check()?;
        std::fs::write(&path, b"data")?;
        Ok(path)
    }
}
struct Media;
#[async_trait]
impl MediaProcessor for Media {
    async fn process(&self, r: ProcessRequest, c: Control) -> Result<(Artifact, Vec<String>)> {
        c.check()?;
        std::fs::copy(r.input, &r.output)?;
        Ok((
            Artifact {
                path: r.output,
                size: 4,
            },
            vec![],
        ))
    }
}
fn engine(d: &std::path::Path) -> Arc<Engine> {
    Engine::new(
        Arc::new(Metadata),
        Arc::new(Download),
        Arc::new(Media),
        d.join("state"),
    )
    .unwrap()
}
fn request(d: &std::path::Path, urls: Vec<&str>) -> DownloadRequest {
    DownloadRequest {
        urls: urls.into_iter().map(str::to_owned).collect(),
        output_dir: d.join("out"),
        format: OutputFormat::Mp3,
        max_height: None,
        chapters: vec![],
        playlist_items: vec![],
    }
}
#[tokio::test]
async fn batch_preserves_success_and_explicit_partial() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let id = e
        .create(request(
            d.path(),
            vec![
                "https://youtu.be/AAAAAAAAAAA",
                "https://youtu.be/BBBBBBBBBBB",
            ],
        ))
        .unwrap();
    let s = e.wait(&id).await.unwrap();
    assert_eq!(s.state, JobState::PartiallyCompleted);
    assert_eq!(s.items[0].state, JobState::Completed);
    assert_eq!(s.items[1].state, JobState::Failed);
    assert_eq!(
        std::fs::read(&s.items[0].artifacts[0].path).unwrap(),
        b"data"
    );
    assert!(e.pause(&id).is_err());
}
#[tokio::test]
async fn next_page_failure_cannot_be_complete() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let id = e
        .create(request(
            d.path(),
            vec!["https://youtube.com/playlist?list=PL1234567890"],
        ))
        .unwrap();
    let s = e.wait(&id).await.unwrap();
    assert_eq!(s.state, JobState::PartiallyCompleted);
    assert!(!s.enumeration_complete);
    assert!(s.error.is_some());
    assert_eq!(s.items.len(), 1);
}
#[tokio::test]
async fn pause_before_worker_starts_then_resume() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let id = e
        .create(request(d.path(), vec!["https://youtu.be/AAAAAAAAAAA"]))
        .unwrap();
    e.pause(&id).unwrap();
    assert_ne!(e.get(&id).unwrap().state, JobState::Paused);
    assert_eq!(e.wait(&id).await.unwrap().state, JobState::Paused);
    e.resume(&id, false).unwrap();
    assert_eq!(e.wait(&id).await.unwrap().state, JobState::Completed);
}
