use super::*;
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tuneka_core::Format;

pub(super) fn source(url: String, length: Option<u64>) -> ResolvedStream {
    ResolvedStream {
        sabr: None,
        segmented: None,
        media_id: "abcdefghijk".into(),
        format: Format {
            id: "140".into(),
            mime_type: "audio/mp4".into(),
            codecs: vec![],
            has_audio: true,
            has_video: false,
            bitrate: 128000,
            height: None,
            content_length: length,
        },
        url,
    }
}
pub(super) fn sink() -> ProgressSink {
    Arc::new(|_| {})
}

pub(super) async fn server(replies: Vec<(&str, &str)>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/media?secret=not-persisted",
        listener.local_addr().unwrap()
    );
    let replies: Vec<(String, String)> = replies
        .into_iter()
        .map(|(a, b)| (a.into(), b.into()))
        .collect();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    tokio::spawn(async move {
        for (headers, body) in replies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                if socket.read_exact(&mut byte).await.is_err() {
                    break;
                }
                request.push(byte[0]);
            }
            captured
                .lock()
                .unwrap()
                .push(String::from_utf8(request).unwrap());
            let _ = socket
                .write_all(
                    format!("HTTP/1.1 {headers}\r\nConnection: close\r\n\r\n{body}").as_bytes(),
                )
                .await;
        }
    });
    (url, requests)
}

#[tokio::test]
async fn complete_200_reuses_matching_receipt_and_refuses_unowned_output() {
    let (url, _) = server(vec![("200 OK\r\nContent-Length: 6", "abcdef")]).await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    let downloader = HttpDownloader::for_local_tests();
    downloader
        .download(
            source(url.clone(), Some(6)),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(&target).await.unwrap(), b"abcdef");
    assert!(!companion(&target, ".part").exists());
    let reused = downloader
        .download(
            source(url.clone(), Some(6)),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap();
    assert_eq!(reused, target);
    tokio::fs::remove_file(companion(&target, ".complete.json"))
        .await
        .unwrap();
    let error = downloader
        .download(
            source(url, Some(6)),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(tokio::fs::read(target).await.unwrap(), b"abcdef");
}

#[tokio::test]
async fn truncated_response_retries_using_validated_range() {
    let (url, requests) = server(vec![
        ("200 OK\r\nContent-Length: 6", "abc"),
        (
            "206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6",
            "def",
        ),
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    HttpDownloader::for_local_tests()
        .download(
            source(url, Some(6)),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(target).await.unwrap(), b"abcdef");
    assert!(requests.lock().unwrap()[1].contains("range: bytes=3-"));
}

async fn paused_download() -> (tempfile::TempDir, PathBuf, String) {
    let (url, _) = server(vec![(
        "206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 0-2/6",
        "abc",
    )])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    let control = Control::default();
    let signal = control.clone();
    let error = HttpDownloader::for_local_tests()
        .download(
            source(url.clone(), Some(6)),
            target.clone(),
            control,
            Arc::new(move |_| signal.pause()),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Paused);
    assert_eq!(
        tokio::fs::read(companion(&target, ".part")).await.unwrap(),
        b"abc"
    );
    let json = tokio::fs::read_to_string(companion(&target, ".checkpoint.json"))
        .await
        .unwrap();
    assert!(!json.contains("secret"));
    assert!(!json.contains("http"));
    (dir, target, url)
}

#[tokio::test]
async fn pause_resumes_after_signed_url_refresh() {
    let (_dir, target, _) = paused_download().await;
    let (url, requests) = server(vec![(
        "206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6",
        "def",
    )])
    .await;
    HttpDownloader::for_local_tests()
        .download(
            source(url, Some(6)),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(target).await.unwrap(), b"abcdef");
    assert!(requests.lock().unwrap()[0].contains("range: bytes=3-"));
}

#[tokio::test]
async fn full_response_to_resume_restarts_instead_of_appending() {
    let (_dir, target, _) = paused_download().await;
    let (url, _) = server(vec![("200 OK\r\nContent-Length: 6", "abcdef")]).await;
    HttpDownloader::for_local_tests()
        .download(
            source(url, Some(6)),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(target).await.unwrap(), b"abcdef");
}

#[tokio::test]
async fn mismatched_range_and_identity_preserve_partial() {
    let (_dir, target, _) = paused_download().await;
    let (url, _) = server(vec![(
        "206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 2-4/6",
        "def",
    )])
    .await;
    let downloader = HttpDownloader::for_local_tests();
    let error = downloader
        .download(
            source(url.clone(), Some(6)),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(
        tokio::fs::read(companion(&target, ".part")).await.unwrap(),
        b"abc"
    );
    let mut changed = source(url, Some(6));
    changed.format.id = "251".into();
    assert_eq!(
        downloader
            .download(changed, target, Control::default(), sink())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn cancellation_removes_checkpoint_and_partial() {
    let (_dir, target, url) = paused_download().await;
    let control = Control::default();
    control.cancel();
    let error = HttpDownloader::for_local_tests()
        .download(source(url, Some(6)), target.clone(), control, sink())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert!(!companion(&target, ".part").exists());
    assert!(!companion(&target, ".checkpoint.json").exists());
}

#[tokio::test]
async fn expiry_is_refreshable_and_urls_are_redacted() {
    let (url, _) = server(vec![("403 Forbidden\r\nContent-Length: 0", "")]).await;
    let dir = tempfile::tempdir().unwrap();
    let error = HttpDownloader::for_local_tests()
        .download(
            source(url, Some(6)),
            dir.path().join("source"),
            Control::default(),
            sink(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(!error.to_string().contains("secret"));
}

#[tokio::test]
async fn rejects_redirect_to_unapproved_host() {
    let (url, _) = server(vec![
        (
            "302 Found\r\nContent-Length: 0\r\nLocation: http://example.com/media?token=private",
            ""
        );
        4
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let error = HttpDownloader::for_local_tests()
        .download(
            source(url, Some(6)),
            dir.path().join("source"),
            Control::default(),
            sink(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Network);
    assert!(!error.to_string().contains("private"));
}

#[tokio::test]
async fn control_interrupts_waiting_for_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server_task = tokio::spawn(async move {
        let (_socket, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
    });
    let control = Control::default();
    let signal = control.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        signal.pause();
    });
    let dir = tempfile::tempdir().unwrap();
    let error = tokio::time::timeout(
        Duration::from_secs(1),
        HttpDownloader::for_local_tests().download(
            source(url, Some(6)),
            dir.path().join("source"),
            control,
            sink(),
        ),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::Paused);
    server_task.abort();
}

#[test]
fn production_urls_reject_non_media_and_credentials() {
    for url in [
        "http://rr1.googlevideo.com/x",
        "https://googlevideo.com.evil.test/x",
        "https://user@rr1.googlevideo.com/x",
        "http://127.0.0.1/x",
        "https://rr1.googlevideo.com:444/x",
    ] {
        assert!(!permitted(&Url::parse(url).unwrap(), false));
    }
    assert!(permitted(
        &Url::parse("https://rr1.googlevideo.com/videoplayback?sig=x").unwrap(),
        false
    ));
}

#[test]
#[ignore = "helper launched by lock_conflict_across_processes"]
fn lock_child() {
    let Ok(path) = std::env::var("TUNEKA_TEST_LOCK") else {
        return;
    };
    let _lock = FileLock::acquire(Path::new(&path)).unwrap();
    std::fs::write(format!("{path}.ready"), b"ready").unwrap();
    std::thread::sleep(Duration::from_secs(15));
}

#[test]
fn lock_conflict_across_processes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.lock");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tests::lock_child", "--ignored"])
        .env("TUNEKA_TEST_LOCK", &path)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let ready = companion(&path, ".ready");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let result = if ready.exists() {
        FileLock::acquire(&path).map(|_| ()).map_err(|e| e.code)
    } else {
        Ok(())
    };
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(ready.exists(), "lock helper did not start");
    assert_eq!(result, Err(ErrorCode::Conflict));
    FileLock::acquire(&path).unwrap();
}

#[tokio::test]
async fn full_response_length_mismatch_does_not_destroy_partial() {
    let (_dir, target, _) = paused_download().await;
    let (url, _) = server(vec![("200 OK\r\nContent-Length: 7", "abcdefg")]).await;
    let error = HttpDownloader::for_local_tests()
        .download(
            source(url, Some(6)),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(
        tokio::fs::read(companion(&target, ".part")).await.unwrap(),
        b"abc"
    );
}

#[tokio::test]
async fn crash_tail_is_discarded_before_resume() {
    let (_dir, target, _) = paused_download().await;
    let mut part = tokio::fs::OpenOptions::new()
        .append(true)
        .open(companion(&target, ".part"))
        .await
        .unwrap();
    part.write_all(b"uncommitted-tail").await.unwrap();
    drop(part);
    let (url, _) = server(vec![(
        "206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6",
        "def",
    )])
    .await;
    HttpDownloader::for_local_tests()
        .download(
            source(url, Some(6)),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(target).await.unwrap(), b"abcdef");
}

#[tokio::test]
async fn retryable_status_recovers_but_exhaustion_never_publishes() {
    let (url, requests) = server(vec![
        ("429 Too Many Requests\r\nContent-Length: 0", ""),
        ("200 OK\r\nContent-Length: 6", "abcdef"),
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    HttpDownloader::for_local_tests()
        .download(
            source(url, Some(6)),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap();
    assert_eq!(requests.lock().unwrap().len(), 2);
    let (url, requests) = server(vec![
        ("503 Service Unavailable\r\nContent-Length: 0", "");
        ATTEMPTS
    ])
    .await;
    let failed = dir.path().join("failed");
    let error = HttpDownloader::for_local_tests()
        .download(
            source(url, Some(6)),
            failed.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Network);
    assert_eq!(requests.lock().unwrap().len(), ATTEMPTS);
    assert!(!failed.exists());
    assert!(companion(&failed, ".checkpoint.json").exists());
}

#[tokio::test]
async fn cancellation_during_stream_cleans_durable_partial() {
    let (url, _) = server(vec![(
        "206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 0-2/6",
        "abc",
    )])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    let control = Control::default();
    let signal = control.clone();
    let error = HttpDownloader::for_local_tests()
        .download(
            source(url, Some(6)),
            target.clone(),
            control,
            Arc::new(move |_| signal.cancel()),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert!(!companion(&target, ".part").exists());
    assert!(!companion(&target, ".checkpoint.json").exists());
    assert!(!target.exists());
}

#[tokio::test]
async fn successful_finite_chunks_do_not_exhaust_retry_budget() {
    const CHUNK: usize = 1024 * 1024;
    let total = 5 * CHUNK + 17;
    let mut replies = Vec::new();
    let mut expected = Vec::new();
    for offset in (0..total).step_by(CHUNK) {
        let end = (offset + CHUNK).min(total) - 1;
        let body =
            String::from_utf8(vec![b'a' + (offset / CHUNK) as u8; end - offset + 1]).unwrap();
        expected.extend_from_slice(body.as_bytes());
        replies.push((format!("206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {offset}-{end}/{total}",body.len()),body));
    }
    let (url, requests) = server(
        replies
            .iter()
            .map(|(headers, body)| (headers.as_str(), body.as_str()))
            .collect(),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("chunked");
    HttpDownloader::for_local_tests()
        .download(
            source(url, Some(total as u64)),
            target.clone(),
            Control::default(),
            sink(),
        )
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(&target).await.unwrap(), expected);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 6);
    assert!(requests.len() > ATTEMPTS);
    for (index, request) in requests.iter().enumerate() {
        let offset = index * CHUNK;
        let end = (offset + CHUNK).min(total) - 1;
        assert!(
            request.contains(&format!("range: bytes={offset}-{end}\r\n")),
            "chunk {index} did not request its finite range"
        );
    }
    assert!(!companion(&target, ".part").exists());
    assert!(!companion(&target, ".checkpoint.json").exists());
}

#[test]
fn released_locks_are_immediately_reacquirable_during_process_spawns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resume.lock");
    let spawner = std::thread::spawn(|| {
        for _ in 0..80 {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "tests::lock_child", "--ignored"])
                .env_remove("TUNEKA_TEST_LOCK")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success());
        }
    });
    let mut conflicts = 0;
    for _ in 0..20_000 {
        match FileLock::acquire(&path) {
            Ok(lock) => drop(lock),
            Err(error) if error.code == ErrorCode::Conflict => conflicts += 1,
            Err(error) => panic!("{error}"),
        }
    }
    spawner.join().unwrap();
    assert_eq!(
        conflicts, 0,
        "dropping a lock must release ownership even during child spawn"
    );
}
