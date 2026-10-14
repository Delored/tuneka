use super::*;
use crate::tests::{server, sink, source};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tuneka_core::Downloader;

fn segmented(urls: Vec<(String, Option<(u64, u64)>)>) -> ResolvedStream {
    let mut stream = source(urls[0].0.clone(), None);
    stream.segmented = Some(SegmentedStream {
        identity: "a".repeat(64),
        segments: urls
            .into_iter()
            .map(|(url, byte_range)| MediaSegment { url, byte_range })
            .collect(),
    });
    stream
}
async fn paused() -> (tempfile::TempDir, PathBuf, ResolvedStream) {
    let (url, requests) = server(vec![("200 OK\r\nContent-Length: 3", "abc")]).await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    let stream = segmented(vec![(url.clone(), None), (url, None)]);
    let control = Control::default();
    let signal = control.clone();
    let events = AtomicUsize::new(0);
    let error = HttpDownloader::for_local_tests()
        .download(
            stream.clone(),
            target.clone(),
            control,
            Arc::new(move |p| {
                if p.bytes == 3 && events.fetch_add(1, Ordering::SeqCst) == 1 {
                    signal.pause();
                }
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Paused);
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert_eq!(
        tokio::fs::read(companion(&target, ".part")).await.unwrap(),
        b"abc"
    );
    let cp: Checkpoint = serde_json::from_slice(
        &tokio::fs::read(companion(&target, ".checkpoint.json"))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!((cp.next_segment, cp.bytes), (1, 3));
    (dir, target, stream)
}

#[tokio::test]
async fn completed_segments_resume_with_refreshed_urls_and_discard_crash_tail() {
    let (_dir, target, mut stream) = paused().await;
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .open(companion(&target, ".part"))
        .await
        .unwrap();
    file.write_all(b"uncommitted").await.unwrap();
    drop(file);
    let (url, requests) = server(vec![("200 OK\r\nContent-Length: 3", "def")]).await;
    for segment in &mut stream.segmented.as_mut().unwrap().segments {
        segment.url = url.clone();
    }
    let downloader = HttpDownloader::for_local_tests();
    downloader
        .download(stream.clone(), target.clone(), Control::default(), sink())
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(&target).await.unwrap(), b"abcdef");
    assert_eq!(requests.lock().unwrap().len(), 1);
    for suffix in [".complete.json"] {
        let json = tokio::fs::read_to_string(companion(&target, suffix))
            .await
            .unwrap();
        assert!(!json.contains("http"));
        assert!(!json.contains("secret"));
    }
    downloader
        .download(stream.clone(), target.clone(), Control::default(), sink())
        .await
        .unwrap();
    assert_eq!(
        requests.lock().unwrap().len(),
        1,
        "completion receipt must avoid network"
    );
    assert!(!companion(&target, ".part").exists());
    assert!(!companion(&target, ".checkpoint.json").exists());
    stream.segmented.as_mut().unwrap().identity = "b".repeat(64);
    assert_eq!(
        downloader
            .download(stream, target.clone(), Control::default(), sink())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        downloader
            .download(source(url, None), target, Control::default(), sink())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn interrupted_segment_retry_never_duplicates_uncommitted_bytes() {
    let (url, requests) = server(vec![
        ("200 OK\r\nContent-Length: 6", "abc"),
        ("200 OK\r\nContent-Length: 6", "abcdef"),
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    HttpDownloader::for_local_tests()
        .download(
            segmented(vec![(url, None)]),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(target).await.unwrap(), b"abcdef");
    assert_eq!(requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn range_segment_is_exact_and_progress_has_known_total() {
    let (url, requests) = server(vec![(
        "206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 2-4/9",
        "abc",
    )])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    HttpDownloader::for_local_tests()
        .download(
            segmented(vec![(url, Some((2, 4)))]),
            target.clone(),
            Control::default(),
            Arc::new(|p| assert_eq!(p.total, Some(3))),
        )
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(target).await.unwrap(), b"abc");
    assert!(requests.lock().unwrap()[0].contains("range: bytes=2-4"));
}

#[tokio::test]
async fn identity_mismatch_and_direct_mode_never_reuse_segment_checkpoint() {
    let (_dir, target, mut stream) = paused().await;
    stream.segmented.as_mut().unwrap().identity = "b".repeat(64);
    let downloader = HttpDownloader::for_local_tests();
    assert_eq!(
        downloader
            .download(stream.clone(), target.clone(), Control::default(), sink())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    stream.segmented = None;
    assert_eq!(
        downloader
            .download(stream, target.clone(), Control::default(), sink())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        tokio::fs::read(companion(&target, ".part")).await.unwrap(),
        b"abc"
    );
}

#[tokio::test]
async fn direct_completion_is_not_a_segment_completion_receipt() {
    let (url, _) = server(vec![("200 OK\r\nContent-Length: 3", "abc")]).await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    let downloader = HttpDownloader::for_local_tests();
    downloader
        .download(
            source(url.clone(), Some(3)),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap();
    assert_eq!(
        downloader
            .download(
                segmented(vec![(url, None)]),
                target,
                Control::default(),
                sink()
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn invalid_segment_responses_are_rejected_without_publication() {
    for (headers, body, requested) in [
        ("200 OK\r\nContent-Length: 3", "abc", Some((2, 4))),
        (
            "206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 1-3/9",
            "abc",
            Some((2, 4)),
        ),
        (
            "206 Partial Content\r\nContent-Length: 2\r\nContent-Range: bytes 2-4/9",
            "ab",
            Some((2, 4)),
        ),
        ("200 OK\r\nContent-Length: 0", "", None),
        (
            "200 OK\r\nContent-Length: 3\r\nContent-Range: bytes 2-4/9",
            "abc",
            None,
        ),
        (
            "200 OK\r\nContent-Length: 3\r\nContent-Encoding: gzip",
            "abc",
            None,
        ),
        ("404 Not Found\r\nContent-Length: 0", "", None),
        ("200 OK\r\nContent-Length: 134217729", "", None),
        ("200 OK\r\nTransfer-Encoding: chunked", "0\r\n\r\n", None),
    ] {
        let (url, _) = server(vec![(headers, body)]).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("source");
        let error = HttpDownloader::for_local_tests()
            .download(
                segmented(vec![(url, requested)]),
                target.clone(),
                Control::default(),
                sink(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Protocol, "{headers}");
        assert!(!target.exists());
        assert_eq!(
            tokio::fs::metadata(companion(&target, ".part"))
                .await
                .unwrap()
                .len(),
            0
        );
    }
}

#[tokio::test]
async fn expiry_is_refreshable_and_retry_budget_is_per_segment() {
    for status in ["401 Unauthorized", "403 Forbidden", "410 Gone"] {
        let headers = format!("{status}\r\nContent-Length: 0");
        let (url, requests) = server(vec![(&headers, "")]).await;
        let dir = tempfile::tempdir().unwrap();
        let error = HttpDownloader::for_local_tests()
            .download(
                segmented(vec![(url, None)]),
                dir.path().join("source"),
                Control::default(),
                sink(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert!(!error.to_string().contains("secret"));
    }
    let (url, requests) = server(vec![
        ("429 Too Many Requests\r\nContent-Length: 0", "");
        ATTEMPTS
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        HttpDownloader::for_local_tests()
            .download(
                segmented(vec![(url, None)]),
                dir.path().join("source"),
                Control::default(),
                sink()
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::Network
    );
    assert_eq!(requests.lock().unwrap().len(), ATTEMPTS);
    let (url, requests) = server(vec![
        ("503 Service Unavailable\r\nContent-Length: 0", ""),
        ("200 OK\r\nContent-Length: 3", "abc"),
        ("503 Service Unavailable\r\nContent-Length: 0", ""),
        ("200 OK\r\nContent-Length: 3", "def"),
    ])
    .await;
    let target = dir.path().join("recovered");
    HttpDownloader::for_local_tests()
        .download(
            segmented(vec![(url.clone(), None), (url, None)]),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(target).await.unwrap(), b"abcdef");
    assert_eq!(requests.lock().unwrap().len(), 4);
}

#[tokio::test]
async fn cancellation_removes_completed_and_inflight_segment_state() {
    let (_dir, target, stream) = paused().await;
    let control = Control::default();
    control.cancel();
    assert_eq!(
        HttpDownloader::for_local_tests()
            .download(stream, target.clone(), control, sink())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    assert!(!companion(&target, ".part").exists());
    assert!(!companion(&target, ".checkpoint.json").exists());
    let (url, _) = server(vec![("200 OK\r\nContent-Length: 3", "abc")]).await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    let control = Control::default();
    let signal = control.clone();
    assert_eq!(
        HttpDownloader::for_local_tests()
            .download(
                segmented(vec![(url, None)]),
                target.clone(),
                control,
                Arc::new(move |_| signal.cancel())
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    assert!(!companion(&target, ".part").exists());
    assert!(!companion(&target, ".checkpoint.json").exists());
    assert!(!target.exists());
}

#[tokio::test]
async fn validate_every_segment_before_any_filesystem_or_network_io() {
    let (url, requests) = server(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("not-created/source");
    let downloader = HttpDownloader::for_local_tests();
    for requested in [Some((4, 3)), Some((0, MAX_SEGMENT)), Some((0, u64::MAX))] {
        assert_eq!(
            downloader
                .download(
                    segmented(vec![(url.clone(), requested)]),
                    target.clone(),
                    Control::default(),
                    sink()
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
    }
    let mut stream = segmented(vec![
        (url.clone(), None),
        ("https://evil.example/media?secret=x".into(), None),
    ]);
    assert_eq!(
        downloader
            .download(stream.clone(), target.clone(), Control::default(), sink())
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    stream.segmented.as_mut().unwrap().segments = vec![
        MediaSegment {
            url,
            byte_range: None
        };
        MAX_SEGMENTS + 1
    ];
    assert_eq!(
        downloader
            .download(stream, target.clone(), Control::default(), sink())
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    assert!(!target.parent().unwrap().exists());
    assert!(requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn valid_chunked_segment_streams_without_known_content_length() {
    let (url, _) = server(vec![(
        "200 OK\r\nTransfer-Encoding: chunked",
        "3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n",
    )])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    HttpDownloader::for_local_tests()
        .download(
            segmented(vec![(url, None)]),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(target).await.unwrap(), b"abcdef");
}

#[tokio::test]
async fn pause_inflight_segment_discards_tail_and_resumes_whole_segment() {
    let (url, _) = server(vec![("200 OK\r\nContent-Length: 3", "abc")]).await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    let control = Control::default();
    let signal = control.clone();
    let error = HttpDownloader::for_local_tests()
        .download(
            segmented(vec![(url, None)]),
            target.clone(),
            control,
            Arc::new(move |_| signal.pause()),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Paused);
    assert_eq!(
        tokio::fs::metadata(companion(&target, ".part"))
            .await
            .unwrap()
            .len(),
        0
    );
    let cp: Checkpoint = serde_json::from_slice(
        &tokio::fs::read(companion(&target, ".checkpoint.json"))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!((cp.next_segment, cp.bytes), (0, 0));
    let (url, _) = server(vec![("200 OK\r\nContent-Length: 3", "abc")]).await;
    HttpDownloader::for_local_tests()
        .download(
            segmented(vec![(url, None)]),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(target).await.unwrap(), b"abc");
}

#[tokio::test]
async fn expired_later_segment_refresh_preserves_completed_prefix() {
    let (url, requests) = server(vec![
        ("200 OK\r\nContent-Length: 3", "abc"),
        ("403 Forbidden\r\nContent-Length: 0", ""),
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    let mut stream = segmented(vec![(url.clone(), None), (url, None)]);
    let downloader = HttpDownloader::for_local_tests();
    assert_eq!(
        downloader
            .download(stream.clone(), target.clone(), Control::default(), sink())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Unavailable
    );
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert_eq!(
        tokio::fs::read(companion(&target, ".part")).await.unwrap(),
        b"abc"
    );
    let (url, requests) = server(vec![("200 OK\r\nContent-Length: 3", "def")]).await;
    for segment in &mut stream.segmented.as_mut().unwrap().segments {
        segment.url = url.clone();
    }
    downloader
        .download(stream, target.clone(), Control::default(), sink())
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(target).await.unwrap(), b"abcdef");
    assert_eq!(requests.lock().unwrap().len(), 1);
}

#[test]
fn large_range_offsets_do_not_overflow_checkpoint_prefix_validation() {
    let stream = segmented(vec![
        ("https://rr.googlevideo.com/a".into(), Some((0, 1))),
        (
            "https://rr.googlevideo.com/b".into(),
            Some((u64::MAX - 2, u64::MAX - 1)),
        ),
    ]);
    let plan = stream.segmented.as_ref().unwrap();
    let cp = Checkpoint {
        version: 2,
        media_id: stream.media_id.clone(),
        format_id: stream.format.id.clone(),
        declared_length: None,
        identity: plan.identity.clone(),
        segment_count: 2,
        next_segment: 2,
        bytes: 4,
    };
    assert!(read_checkpoint(&serde_json::to_vec(&cp).unwrap(), &stream, plan).is_ok());
}
