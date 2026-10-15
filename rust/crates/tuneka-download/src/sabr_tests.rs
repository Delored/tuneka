use super::*;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tuneka_core::Downloader;

fn format(itag: u32, length: u64) -> SabrFormat {
    SabrFormat {
        itag,
        last_modified: 123,
        xtags: None,
        content_length: length,
    }
}
fn stream(url: String) -> ResolvedStream {
    let mut stream = crate::tests::source(url, Some(9));
    stream.format.id = "sabr-140".into();
    stream.sabr = Some(SabrStream {
        client_version: "test-version".into(),
        user_agent: "Tuneka-test".into(),
        video_playback_config: b"private-config".to_vec(),
        po_token: None,
        video: format(160, 9),
        audio: format(140, 9),
        selected_itag: 140,
        duration_ms: 2000,
    });
    stream
}
fn segment(itag: u32, sequence: u32, offset: u64, initialization: bool, body: &[u8]) -> Vec<u8> {
    let mut header = Vec::new();
    for (field, value) in [
        (1, u64::from(itag)),
        (3, u64::from(itag)),
        (4, 123),
        (6, offset),
        (8, u64::from(initialization)),
        (9, u64::from(sequence)),
        (11, u64::from(sequence.saturating_sub(1)) * 1000),
        (12, if initialization { 0 } else { 1000 }),
        (14, body.len() as u64),
    ] {
        proto::uint(&mut header, field, value);
    }
    proto::bytes(&mut header, 2, b"abcdefghijk");
    let mut result = proto::frame(20, &header);
    // The MEDIA header association is raw uint8, unlike part framing.
    let prefix = vec![itag as u8];
    let mut media = prefix.clone();
    media.extend_from_slice(body);
    result.extend(proto::frame(21, &media));
    result.extend(proto::frame(22, &prefix));
    result
}
fn protection(status: u64) -> Vec<u8> {
    let mut data = Vec::new();
    proto::uint(&mut data, 1, status);
    proto::frame(58, &data)
}
fn first() -> Vec<u8> {
    let mut data = protection(2);
    data.extend(segment(160, 0, 0, true, b"VID"));
    data.extend(segment(140, 0, 0, true, b"INI"));
    data.extend(segment(160, 1, 3, false, b"vvv"));
    data.extend(segment(140, 1, 3, false, b"abc"));
    let mut policy = Vec::new();
    proto::bytes(&mut policy, 7, b"private-cookie");
    data.extend(proto::frame(35, &policy));
    data
}
fn last() -> Vec<u8> {
    let mut data = protection(2);
    data.extend(segment(160, 2, 6, false, b"www"));
    data.extend(segment(140, 2, 6, false, b"def"));
    data
}

async fn server(replies: Vec<(u16, Vec<u8>, Option<usize>)>) -> (String, Arc<Mutex<Vec<Vec<u8>>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/videoplayback?n=private-transform",
        listener.local_addr().unwrap()
    );
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    tokio::spawn(async move {
        for (status, body, declared) in replies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                if socket.read_exact(&mut byte).await.is_err() {
                    return;
                }
                headers.push(byte[0]);
                if headers.len() > 65536 {
                    return;
                }
            }
            let text = String::from_utf8(headers.clone()).unwrap();
            let length = text
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|s| s.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            let mut payload = vec![0; length];
            if socket.read_exact(&mut payload).await.is_err() {
                return;
            }
            headers.extend_from_slice(&payload);
            captured.lock().unwrap().push(headers);
            let reason = if status == 200 { "OK" } else { "Error" };
            let head = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/vnd.yt-ump\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                declared.unwrap_or(body.len())
            );
            if socket.write_all(head.as_bytes()).await.is_ok() {
                let _ = socket.write_all(&body).await;
            }
        }
    });
    (url, requests)
}
fn reply(body: Vec<u8>) -> (u16, Vec<u8>, Option<usize>) {
    (200, body, None)
}

#[test]
fn ump_and_protobuf_bounds_and_truncation_are_strict() {
    for length in [0, 127, 128, 16383, 16384, 2_097_152] {
        let bytes = vec![3; length];
        let frame = proto::frame(20, &bytes);
        let parts = proto::parts(&frame).unwrap();
        assert_eq!(parts[0].1, bytes);
    }
    assert_eq!(
        proto::parts(&[20, 128]).unwrap_err().code,
        ErrorCode::Network
    );
    assert_eq!(
        proto::parts(&[248, 0]).unwrap_err().code,
        ErrorCode::Protocol
    );
    assert!(Message::parse(&[0]).is_err());
    assert!(Message::parse(&[8, 255, 255, 255, 255, 255, 255, 255, 255, 255, 2]).is_err());
    assert!(Message::parse(&vec![0; 128 * 1024 + 1]).is_err());
    assert!(Message::parse(&[8, 1, 8, 2]).unwrap().uint(1).is_err());
}
#[test]
fn complete_media_and_obfuscated_session_metadata_are_separate() {
    let source = stream("https://rr.googlevideo.com/videoplayback".into());
    let config = source.sabr.as_ref().unwrap();
    let saved = Checkpoint::new(&source, config);
    let response = parse_response(&first(), &saved, &Control::default()).unwrap();
    assert_eq!(response.segments.len(), 4);
    assert!(response.pending_protection);
    assert_eq!(response.cookie.unwrap(), b"private-cookie");
    assert_eq!(response.segments[3].data, b"abc");
    assert!(parse_response(&protection(3), &saved, &Control::default()).is_err());
    assert!(parse_response(&proto::frame(44, b""), &saved, &Control::default()).is_err());
    assert!(parse_response(&proto::frame(12, b"encrypted"), &saved, &Control::default()).is_err());
    let mut truncated = first();
    truncated.pop();
    assert_eq!(
        parse_response(&truncated, &saved, &Control::default())
            .err()
            .unwrap()
            .code,
        ErrorCode::Network
    );
}
#[test]
fn header_xtags_and_partial_segment_selectors_fail_closed() {
    let source = stream("https://rr.googlevideo.com/videoplayback".into());
    let saved = Checkpoint::new(&source, source.sabr.as_ref().unwrap());
    let encoded = segment(140, 1, 3, false, b"abc");
    let header = proto::parts(&encoded).unwrap()[0].1;
    let mut changed = header.to_vec();
    proto::bytes(&mut changed, 5, b"different-language");
    assert_eq!(
        parse_header(&changed, &saved).err().unwrap().code,
        ErrorCode::Conflict
    );
    for (field, value) in [(17, 1), (18, 2)] {
        let mut changed = header.to_vec();
        proto::uint(&mut changed, field, value);
        assert_eq!(
            parse_header(&changed, &saved).err().unwrap().code,
            ErrorCode::Unsupported
        );
    }
}
#[tokio::test]
async fn actual_post_continuation_publishes_only_selected_track_and_reuses_receipt() {
    let (url, requests) = server(vec![reply(first()), reply(last())]).await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    let mut source = stream(url);
    source.sabr.as_mut().unwrap().po_token = Some(b"private-proof-token".to_vec());
    let downloader = HttpDownloader::for_local_tests();
    downloader
        .download(
            source.clone(),
            target.clone(),
            Control::default(),
            crate::tests::sink(),
        )
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(&target).await.unwrap(), b"INIabcdef");
    assert_eq!(requests.lock().unwrap().len(), 2);
    {
        let captured = requests.lock().unwrap();
        let request = &captured[1];
        let start = request.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let body = Message::parse(&request[start..]).unwrap();
        assert!(
            body.bytes(19)
                .unwrap()
                .unwrap()
                .windows(14)
                .any(|w| w == b"private-cookie")
        );
        assert!(
            String::from_utf8_lossy(&captured[0])
                .starts_with("POST /videoplayback?n=private-transform&rn=0 ")
        );
        assert!(
            String::from_utf8_lossy(&captured[1])
                .starts_with("POST /videoplayback?n=private-transform&rn=1 ")
        );
    }
    let receipt = tokio::fs::read_to_string(companion(&target, ".complete.json"))
        .await
        .unwrap();
    for secret in [
        "http",
        "private-transform",
        "private-cookie",
        "private-config",
        "private-proof-token",
        "test-version",
    ] {
        assert!(!receipt.contains(secret));
    }
    downloader
        .download(source, target, Control::default(), crate::tests::sink())
        .await
        .unwrap();
    assert_eq!(requests.lock().unwrap().len(), 2);
}
async fn paused() -> (tempfile::TempDir, PathBuf, ResolvedStream) {
    let (url, _) = server(vec![reply(first())]).await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("source");
    let source = stream(url);
    let control = Control::default();
    let signal = control.clone();
    let progress = Arc::new(move |p: Progress| {
        if p.bytes == 6 {
            signal.pause();
        }
    });
    assert_eq!(
        HttpDownloader::for_local_tests()
            .download(source.clone(), target.clone(), control, progress)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Paused
    );
    assert_eq!(
        tokio::fs::read(companion(&target, ".part")).await.unwrap(),
        b"INIabc"
    );
    (dir, target, source)
}
#[tokio::test]
async fn pause_resume_refreshes_url_discards_crash_tail_and_reports_only_real_ranges() {
    let (_dir, target, mut source) = paused().await;
    let mut part = tokio::fs::OpenOptions::new()
        .append(true)
        .open(companion(&target, ".part"))
        .await
        .unwrap();
    part.write_all(b"crash-tail").await.unwrap();
    drop(part);
    let (url, requests) = server(vec![reply(last())]).await;
    source.url = url;
    HttpDownloader::for_local_tests()
        .download(
            source,
            target.clone(),
            Control::default(),
            crate::tests::sink(),
        )
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(target).await.unwrap(), b"INIabcdef");
    let captured = requests.lock().unwrap();
    let request = &captured[0];
    let start = request.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    assert!(!request.windows(14).any(|w| w == b"private-cookie"));
    let body = Message::parse(&request[start..]).unwrap();
    let abr = Message::parse(body.bytes(1).unwrap().unwrap()).unwrap();
    assert_eq!(abr.uint(28).unwrap(), Some(1000));
}
#[tokio::test]
async fn changed_revision_or_transport_never_reuses_partial() {
    let (_dir, target, mut source) = paused().await;
    source.sabr.as_mut().unwrap().audio.last_modified += 1;
    assert_eq!(
        HttpDownloader::for_local_tests()
            .download(
                source,
                target.clone(),
                Control::default(),
                crate::tests::sink()
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        tokio::fs::read(companion(&target, ".part")).await.unwrap(),
        b"INIabc"
    );
}
#[tokio::test]
async fn corrupt_committed_prefix_is_rejected_before_fresh_session_skips_it() {
    let (_dir, target, mut source) = paused().await;
    tokio::fs::write(companion(&target, ".part"), b"INIxyz")
        .await
        .unwrap();
    let (url, requests) = server(vec![reply(last())]).await;
    source.url = url;
    assert_eq!(
        HttpDownloader::for_local_tests()
            .download(
                source,
                target.clone(),
                Control::default(),
                crate::tests::sink()
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert!(requests.lock().unwrap().is_empty());
    assert!(!target.exists());
}
#[tokio::test]
async fn interrupted_http_or_ump_response_retries_without_duplicate_bytes() {
    for malformed_http in [true, false] {
        let full = first();
        let mut partial = full.clone();
        partial.pop();
        let declared = malformed_http.then_some(full.len());
        let (url, requests) =
            server(vec![(200, partial, declared), reply(full), reply(last())]).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("source");
        HttpDownloader::for_local_tests()
            .download(
                stream(url),
                target.clone(),
                Control::default(),
                crate::tests::sink(),
            )
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(target).await.unwrap(), b"INIabcdef");
        assert_eq!(requests.lock().unwrap().len(), 3);
        for (number, request) in requests.lock().unwrap().iter().enumerate() {
            assert!(String::from_utf8_lossy(request).starts_with(&format!(
                "POST /videoplayback?n=private-transform&rn={number} "
            )));
        }
    }
}
#[tokio::test]
async fn duplicate_segments_compare_actual_bytes_and_changed_content_is_rejected() {
    for changed in [false, true] {
        let (_dir, target, mut source) = paused().await;
        let mut repeated = segment(140, 1, 3, false, if changed { b"xyz" } else { b"abc" });
        repeated.extend(last());
        let (url, _) = server(vec![reply(repeated)]).await;
        source.url = url;
        let result = HttpDownloader::for_local_tests()
            .download(
                source,
                target.clone(),
                Control::default(),
                crate::tests::sink(),
            )
            .await;
        if changed {
            assert_eq!(result.unwrap_err().code, ErrorCode::Conflict);
            assert!(!target.exists());
        } else {
            result.unwrap();
            assert_eq!(tokio::fs::read(target).await.unwrap(), b"INIabcdef");
        }
    }
}
#[tokio::test]
async fn protection_errors_stalls_and_gaps_never_publish_prefix_as_complete() {
    let mut gap = first();
    gap.extend(segment(140, 2, 7, false, b"ef"));
    for replies in [
        vec![reply(protection(3))],
        vec![reply(proto::frame(44, b""))],
        vec![reply(protection(2)); 3],
        vec![reply(gap)],
    ] {
        let (url, _) = server(replies).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("source");
        assert!(
            HttpDownloader::for_local_tests()
                .download(
                    stream(url),
                    target.clone(),
                    Control::default(),
                    crate::tests::sink()
                )
                .await
                .is_err()
        );
        assert!(!target.exists());
    }
}
#[tokio::test]
async fn cancellation_cleans_committed_checkpoint_and_partial() {
    let (_dir, target, source) = paused().await;
    let control = Control::default();
    control.cancel();
    assert_eq!(
        HttpDownloader::for_local_tests()
            .download(source, target.clone(), control, crate::tests::sink())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    assert!(!companion(&target, ".part").exists());
    assert!(!companion(&target, ".checkpoint.json").exists());
}
#[test]
fn initialization_uses_proto_defaults_but_media_requires_sequence_and_offset() {
    let source = stream("https://rr.googlevideo.com/videoplayback".into());
    let saved = Checkpoint::new(&source, source.sabr.as_ref().unwrap());
    let mut header = Vec::new();
    for (field, value) in [(1, 1), (3, 140), (4, 123), (8, 1), (14, 3)] {
        proto::uint(&mut header, field, value);
    }
    let (_, parsed) = parse_header(&header, &saved).unwrap();
    assert!(parsed.completed.initialization);
    assert_eq!(parsed.completed.sequence, 0);
    assert_eq!(parsed.completed.offset, 0);
    let mut media = Vec::new();
    for (field, value) in [(1, 1), (3, 140), (4, 123), (12, 1000), (14, 3)] {
        proto::uint(&mut media, field, value);
    }
    assert!(parse_header(&media, &saved).is_err());
}
#[test]
fn session_contexts_preserve_activation_and_apply_wire_order() {
    let mut session = Session::default();
    apply_contexts(
        &mut session,
        vec![
            ContextAction::Update(
                1,
                Context {
                    value: b"old".to_vec(),
                    active: true,
                },
                false,
            ),
            ContextAction::Update(
                1,
                Context {
                    value: b"new".to_vec(),
                    active: false,
                },
                false,
            ),
        ],
    )
    .unwrap();
    assert!(session.contexts[&1].active);
    assert_eq!(session.contexts[&1].value, b"new");
    apply_contexts(
        &mut session,
        vec![ContextAction::Stop(1), ContextAction::Start(1)],
    )
    .unwrap();
    assert!(session.contexts[&1].active);
    apply_contexts(
        &mut session,
        vec![
            ContextAction::Discard(1),
            ContextAction::Update(
                1,
                Context {
                    value: b"replacement".to_vec(),
                    active: true,
                },
                false,
            ),
        ],
    )
    .unwrap();
    assert_eq!(session.contexts[&1].value, b"replacement");
}
#[test]
fn raw_uint8_header_id_does_not_consume_first_media_byte() {
    let mut source = stream("https://rr.googlevideo.com/videoplayback".into());
    source.sabr.as_mut().unwrap().audio.itag = 200;
    source.sabr.as_mut().unwrap().selected_itag = 200;
    let saved = Checkpoint::new(&source, source.sabr.as_ref().unwrap());
    let body = segment(200, 0, 0, true, &[255, 0, 7]);
    let parsed = parse_response(&body, &saved, &Control::default()).unwrap();
    assert_eq!(parsed.segments[0].data, [255, 0, 7]);
}
#[test]
fn wire_policy_remembers_activation_before_value_and_across_discard() {
    let source = stream("https://rr.googlevideo.com/videoplayback".into());
    let saved = Checkpoint::new(&source, source.sabr.as_ref().unwrap());
    let policy = |field| {
        let mut value = Vec::new();
        proto::uint(&mut value, field, 7);
        proto::frame(59, &value)
    };
    let update = |value: &[u8]| {
        let mut out = Vec::new();
        proto::uint(&mut out, 1, 7);
        proto::bytes(&mut out, 3, value);
        proto::frame(57, &out)
    };
    let mut body = policy(1);
    body.extend(update(b"first"));
    body.extend(policy(3));
    body.extend(update(b"second"));
    let parsed = parse_response(&body, &saved, &Control::default()).unwrap();
    let mut session = Session::default();
    apply_contexts(&mut session, parsed.context_actions).unwrap();
    assert!(session.contexts[&7].active);
    assert_eq!(session.contexts[&7].value, b"second");
    let body = request(&saved, source.sabr.as_ref().unwrap(), &session, 0);
    let parsed = Message::parse(&body).unwrap();
    let context = Message::parse(parsed.bytes(19).unwrap().unwrap()).unwrap();
    let sent = Message::parse(context.bytes(5).unwrap().unwrap()).unwrap();
    assert_eq!(sent.bytes(2).unwrap(), Some(b"second".as_slice()));
    apply_contexts(
        &mut session,
        vec![
            ContextAction::Stop(7),
            ContextAction::Discard(7),
            ContextAction::Update(
                7,
                Context {
                    value: b"third".to_vec(),
                    active: false,
                },
                false,
            ),
        ],
    )
    .unwrap();
    assert!(!session.contexts[&7].active);
}
#[test]
fn buffered_time_range_mirrors_only_completed_real_segments() {
    let source = stream("https://rr.googlevideo.com/videoplayback".into());
    let config = source.sabr.as_ref().unwrap();
    let mut saved = Checkpoint::new(&source, config);
    for segment in parse_response(&first(), &saved, &Control::default())
        .unwrap()
        .segments
    {
        if segment.itag == 140 {
            saved.audio_track.segments.push(segment.completed);
        }
    }
    let body = request(&saved, config, &Session::default(), 123);
    let parsed = Message::parse(&body).unwrap();
    let range = Message::parse(parsed.bytes(3).unwrap().unwrap()).unwrap();
    assert_eq!(range.uint(2).unwrap(), Some(0));
    assert_eq!(range.uint(3).unwrap(), Some(1000));
    assert_eq!(range.uint(4).unwrap(), Some(1));
    assert_eq!(range.uint(5).unwrap(), Some(1));
    let time = Message::parse(range.bytes(6).unwrap().unwrap()).unwrap();
    assert_eq!(time.uint(1).unwrap(), Some(0));
    assert_eq!(time.uint(2).unwrap(), Some(1000));
    assert_eq!(time.uint(3).unwrap(), Some(1000));
    let abr = Message::parse(parsed.bytes(1).unwrap().unwrap()).unwrap();
    assert_eq!(
        abr.uint(28).unwrap(),
        Some(0),
        "unknown peer timeline is not fabricated"
    );
}
#[test]
fn request_counter_replaces_only_rn_and_preserves_signed_values() {
    let original = Url::parse(
        "https://rr.googlevideo.com/videoplayback?n=abc%2Fdef&sig=one%2Btwo&rn=99&rn=100",
    )
    .unwrap();
    let numbered = numbered_url(&original, 7);
    let pairs: Vec<_> = numbered.query_pairs().collect();
    assert_eq!(pairs.iter().filter(|(k, _)| k == "rn").count(), 1);
    assert!(pairs.iter().any(|(k, v)| k == "rn" && v == "7"));
    assert!(pairs.iter().any(|(k, v)| k == "n" && v == "abc/def"));
    assert!(pairs.iter().any(|(k, v)| k == "sig" && v == "one+two"));
}
#[test]
fn optional_proof_token_is_raw_wire_bytes_and_never_checkpoint_state() {
    let mut source = stream("https://rr.googlevideo.com/videoplayback".into());
    let read_token = |source: &ResolvedStream| {
        let config = source.sabr.as_ref().unwrap();
        let saved = Checkpoint::new(source, config);
        let body = request(&saved, config, &Session::default(), 0);
        let parsed = Message::parse(&body).unwrap();
        let context = Message::parse(parsed.bytes(19).unwrap().unwrap()).unwrap();
        context.bytes(2).unwrap().map(Vec::from)
    };
    assert_eq!(read_token(&source), None);
    let token = vec![0, 1, 127, 128, 255];
    source.sabr.as_mut().unwrap().po_token = Some(token.clone());
    assert_eq!(read_token(&source), Some(token));
    let saved = Checkpoint::new(&source, source.sabr.as_ref().unwrap());
    assert!(!serde_json::to_string(&saved).unwrap().contains("po_token"));
    for token in [Vec::new(), vec![1; 16 * 1024 + 1]] {
        source.sabr.as_mut().unwrap().po_token = Some(token);
        assert!(validate(&source, source.sabr.as_ref().unwrap(), false).is_err());
    }
}
