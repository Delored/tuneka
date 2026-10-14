//! Clear finite-VOD SABR download. Opaque session state stays in RAM; only
//! completed, identity-bound media segments become durable resume state.
mod proto;
use super::{ATTEMPTS, FileLock, HttpDownloader, atomic_json, companion, controlled, permitted};
use futures_util::StreamExt;
use proto::Message;
use reqwest::{StatusCode, Url, header};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tuneka_core::{
    Control, Error, ErrorCode, Progress, ProgressSink, ResolvedStream, Result, SabrFormat,
    SabrStream,
};

const MAX_RESPONSE: usize = 8 * 1024 * 1024;
const MAX_SEGMENT: u64 = 128 * 1024 * 1024;
const MAX_SEGMENTS: usize = 20_000;
const MAX_DURATION: u64 = 7 * 24 * 60 * 60 * 1000;
fn protocol(message: &str) -> Error {
    Error::new(ErrorCode::Protocol, message)
}
fn unavailable(message: &str) -> Error {
    Error::new(ErrorCode::Unavailable, message)
}
fn conflict(message: &str) -> Error {
    Error::new(ErrorCode::Conflict, message)
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Identity {
    itag: u32,
    last_modified: u64,
    xtags: Option<String>,
    content_length: u64,
}
impl From<&SabrFormat> for Identity {
    fn from(value: &SabrFormat) -> Self {
        Self {
            itag: value.itag,
            last_modified: value.last_modified,
            xtags: value.xtags.clone(),
            content_length: value.content_length,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Completed {
    sequence: u32,
    initialization: bool,
    offset: u64,
    length: u64,
    start_ms: u64,
    duration_ms: u64,
    sha256: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Track {
    segments: Vec<Completed>,
}
impl Track {
    fn bytes(&self) -> u64 {
        self.segments.last().map_or(0, |s| s.offset + s.length)
    }
    fn initialized(&self) -> bool {
        self.segments.first().is_some_and(|s| s.initialization)
    }
    fn end_ms(&self) -> u64 {
        self.segments
            .last()
            .map_or(0, |s| s.start_ms + s.duration_ms)
    }
    fn media(&self) -> impl Iterator<Item = &Completed> {
        self.segments.iter().filter(|s| !s.initialization)
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Checkpoint {
    version: u8,
    media_id: String,
    format_id: String,
    selected_itag: u32,
    video: Identity,
    audio: Identity,
    duration_ms: u64,
    video_track: Track,
    audio_track: Track,
}
impl Checkpoint {
    fn new(stream: &ResolvedStream, config: &SabrStream) -> Self {
        Self {
            version: 3,
            media_id: stream.media_id.clone(),
            format_id: stream.format.id.clone(),
            selected_itag: config.selected_itag,
            video: (&config.video).into(),
            audio: (&config.audio).into(),
            duration_ms: config.duration_ms,
            video_track: Track::default(),
            audio_track: Track::default(),
        }
    }
    fn track(&self, itag: u32) -> &Track {
        if itag == self.video.itag {
            &self.video_track
        } else {
            &self.audio_track
        }
    }
    fn track_mut(&mut self, itag: u32) -> &mut Track {
        if itag == self.video.itag {
            &mut self.video_track
        } else {
            &mut self.audio_track
        }
    }
    fn selected(&self) -> &Track {
        self.track(self.selected_itag)
    }
    fn identity(&self, itag: u32) -> Result<&Identity> {
        if itag == self.video.itag {
            Ok(&self.video)
        } else if itag == self.audio.itag {
            Ok(&self.audio)
        } else {
            Err(protocol("SABR server selected an unexpected format"))
        }
    }
}
fn validate(stream: &ResolvedStream, config: &SabrStream, allow_loopback: bool) -> Result<Url> {
    let url = Url::parse(&stream.url).map_err(|_| protocol("invalid SABR URL"))?;
    if stream.url.len() > 65536 || !permitted(&url, allow_loopback) || stream.segmented.is_some() {
        return Err(protocol("invalid SABR transport or URL host"));
    }
    if config.video.itag == config.audio.itag
        || ![config.video.itag, config.audio.itag].contains(&config.selected_itag)
        || config.client_version.is_empty()
        || config.client_version.len() > 128
        || config.user_agent.is_empty()
        || config.user_agent.len() > 4096
        || config.video_playback_config.is_empty()
        || config.video_playback_config.len() > 65536
        || config
            .po_token
            .as_ref()
            .is_some_and(|token| token.is_empty() || token.len() > 16 * 1024)
        || config.duration_ms == 0
        || config.duration_ms > MAX_DURATION
    {
        return Err(protocol("invalid SABR session inputs"));
    }
    for format in [&config.video, &config.audio] {
        if format.content_length == 0
            || format.content_length > MAX_SEGMENT * MAX_SEGMENTS as u64
            || format.itag == 0
            || format.last_modified == 0
            || format.xtags.as_ref().is_some_and(|s| s.len() > 4096)
        {
            return Err(protocol("invalid SABR format identity"));
        }
    }
    let selected_length = if config.selected_itag == config.video.itag {
        config.video.content_length
    } else {
        config.audio.content_length
    };
    if stream
        .format
        .content_length
        .is_some_and(|n| n != selected_length)
    {
        return Err(protocol("SABR declared stream length mismatch"));
    }
    Ok(url)
}
fn validate_track(track: &Track, identity: &Identity, duration: u64) -> Result<()> {
    if track.segments.len() > MAX_SEGMENTS {
        return Err(protocol("SABR checkpoint segment limit exceeded"));
    }
    let mut bytes = 0_u64;
    let mut last_sequence = 0;
    let mut last_start = 0;
    let mut last_end = 0;
    for (index, segment) in track.segments.iter().enumerate() {
        if segment.length == 0
            || segment.length > MAX_SEGMENT
            || segment.offset != bytes
            || segment.initialization != (index == 0)
            || (index == 0 && segment.sequence != 0)
            || (index > 0 && segment.sequence != last_sequence + 1)
            || segment.start_ms < last_start
            || segment.start_ms.abs_diff(last_end) > 2
            || segment.duration_ms > MAX_DURATION
            || segment.start_ms.saturating_add(segment.duration_ms) > duration.saturating_add(1000)
            || segment.sha256.len() != 64
            || !segment.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(protocol("invalid SABR checkpoint segment boundary"));
        }
        bytes = bytes
            .checked_add(segment.length)
            .filter(|n| *n <= identity.content_length)
            .ok_or_else(|| protocol("SABR checkpoint byte count overflow"))?;
        last_sequence = segment.sequence;
        last_start = segment.start_ms;
        last_end = segment.start_ms + segment.duration_ms;
    }
    Ok(())
}
async fn read_checkpoint(
    path: &Path,
    stream: &ResolvedStream,
    config: &SabrStream,
) -> Result<Checkpoint> {
    if tokio::fs::metadata(path).await?.len() > MAX_RESPONSE as u64 {
        return Err(protocol("SABR checkpoint size limit exceeded"));
    }
    let value: serde_json::Value = serde_json::from_slice(&tokio::fs::read(path).await?)?;
    if value.get("version").and_then(serde_json::Value::as_u64) != Some(3) {
        return Err(conflict("checkpoint download mode changed"));
    }
    let saved: Checkpoint = serde_json::from_value(value)?;
    if saved.media_id != stream.media_id
        || saved.format_id != stream.format.id
        || saved.selected_itag != config.selected_itag
        || saved.video != Identity::from(&config.video)
        || saved.audio != Identity::from(&config.audio)
        || saved.duration_ms != config.duration_ms
    {
        return Err(conflict("SABR resume identity changed"));
    }
    validate_track(&saved.video_track, &saved.video, config.duration_ms)?;
    validate_track(&saved.audio_track, &saved.audio, config.duration_ms)?;
    Ok(saved)
}

pub(super) async fn download(
    downloader: &HttpDownloader,
    stream: ResolvedStream,
    target: PathBuf,
    control: Control,
    progress: ProgressSink,
) -> Result<PathBuf> {
    let config = stream.sabr.as_ref().expect("SABR dispatch");
    let url = validate(&stream, config, downloader.allow_loopback)?;
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let _lock = FileLock::acquire(&companion(&target, ".lock"))?;
    if let Err(error) = control.check() {
        if error.code == ErrorCode::Cancelled {
            cleanup(&target).await?;
        }
        return Err(error);
    }
    let expected = if config.selected_itag == config.video.itag {
        config.video.content_length
    } else {
        config.audio.content_length
    };
    if target.exists() {
        let saved = read_checkpoint(&companion(&target, ".complete.json"), &stream, config)
            .await
            .map_err(|_| conflict("download target has no matching SABR completion receipt"))?;
        if saved.selected().bytes() == expected
            && tokio::fs::metadata(&target).await?.len() == expected
        {
            return Ok(target);
        }
        return Err(conflict(
            "download target does not match SABR completion receipt",
        ));
    }
    let result = run(
        downloader, &stream, config, url, &target, &control, &progress,
    )
    .await;
    if result
        .as_ref()
        .is_err_and(|e| e.code == ErrorCode::Cancelled)
    {
        cleanup(&target).await?;
    }
    result
}
async fn cleanup(target: &Path) -> Result<()> {
    for suffix in [".part", ".checkpoint.json"] {
        match tokio::fs::remove_file(companion(target, suffix)).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
async fn verify_partial(
    file: &mut tokio::fs::File,
    track: &Track,
    control: &Control,
) -> Result<()> {
    let mut buffer = vec![0; 64 * 1024];
    for segment in &track.segments {
        file.seek(std::io::SeekFrom::Start(segment.offset)).await?;
        let mut remaining = segment.length;
        let mut hash = Sha256::new();
        while remaining > 0 {
            control.check()?;
            let length = (remaining as usize).min(buffer.len());
            file.read_exact(&mut buffer[..length]).await?;
            hash.update(&buffer[..length]);
            remaining -= length as u64;
        }
        if format!("{:x}", hash.finalize()) != segment.sha256 {
            return Err(conflict("SABR partial file content changed"));
        }
    }
    Ok(())
}
#[derive(Default)]
struct Session {
    cookie: Vec<u8>,
    contexts: BTreeMap<u32, Context>,
    active_contexts: BTreeSet<u32>,
    redirects: usize,
    request_number: u64,
}
struct Context {
    value: Vec<u8>,
    active: bool,
}
enum ContextAction {
    Update(u32, Context, bool),
    Start(u32),
    Stop(u32),
    Discard(u32),
}
fn apply_contexts(session: &mut Session, actions: Vec<ContextAction>) -> Result<()> {
    for action in actions {
        match action {
            ContextAction::Update(kind, mut context, keep) => {
                if keep && session.contexts.contains_key(&kind) {
                    continue;
                }
                if context.active {
                    session.active_contexts.insert(kind);
                }
                context.active = session.active_contexts.contains(&kind);
                session.contexts.insert(kind, context);
            }
            ContextAction::Start(kind) => {
                session.active_contexts.insert(kind);
                if let Some(context) = session.contexts.get_mut(&kind) {
                    context.active = true;
                }
            }
            ContextAction::Stop(kind) => {
                session.active_contexts.remove(&kind);
                if let Some(context) = session.contexts.get_mut(&kind) {
                    context.active = false;
                }
            }
            ContextAction::Discard(kind) => {
                session.contexts.remove(&kind);
            }
        }
        if session.contexts.len() > 64 || session.active_contexts.len() > 64 {
            return Err(protocol("SABR context count limit exceeded"));
        }
    }
    Ok(())
}
fn format_id(identity: &Identity) -> Vec<u8> {
    let mut out = Vec::new();
    proto::uint(&mut out, 1, u64::from(identity.itag));
    proto::uint(&mut out, 2, identity.last_modified);
    if let Some(xtags) = &identity.xtags {
        proto::bytes(&mut out, 3, xtags.as_bytes());
    }
    out
}
fn request(saved: &Checkpoint, config: &SabrStream, session: &Session, elapsed: u64) -> Vec<u8> {
    let mut abr = Vec::new();
    for (field, value) in [
        (
            28,
            saved.video_track.end_ms().min(saved.audio_track.end_ms()),
        ),
        (29, elapsed),
        (34, 1),
        (36, elapsed),
        (39, elapsed),
        (40, 3),
    ] {
        proto::uint(&mut abr, field, value);
    }
    proto::float(&mut abr, 35, 1.0);
    let mut client = Vec::new();
    proto::uint(&mut client, 16, 1);
    proto::bytes(&mut client, 17, config.client_version.as_bytes());
    proto::bytes(&mut client, 1, b"en");
    let mut context = Vec::new();
    proto::bytes(&mut context, 1, &client);
    if let Some(token) = &config.po_token {
        proto::bytes(&mut context, 2, token);
    }
    if !session.cookie.is_empty() {
        proto::bytes(&mut context, 3, &session.cookie);
    }
    for (kind, state) in &session.contexts {
        if state.active {
            let mut value = Vec::new();
            proto::uint(&mut value, 1, u64::from(*kind));
            proto::bytes(&mut value, 2, &state.value);
            proto::bytes(&mut context, 5, &value);
        } else {
            proto::uint(&mut context, 6, u64::from(*kind));
        }
    }
    let mut out = Vec::new();
    proto::bytes(&mut out, 1, &abr);
    proto::bytes(&mut out, 5, &config.video_playback_config);
    proto::bytes(&mut out, 16, &format_id(&saved.audio));
    proto::bytes(&mut out, 17, &format_id(&saved.video));
    proto::bytes(&mut out, 19, &context);
    for identity in [&saved.video, &saved.audio] {
        let track = saved.track(identity.itag);
        if track.initialized() {
            proto::bytes(&mut out, 2, &format_id(identity));
        }
        if let (Some(first), Some(last)) = (track.media().next(), track.media().last()) {
            let duration_ms = last.start_ms + last.duration_ms - first.start_ms;
            let mut buffered = Vec::new();
            proto::bytes(&mut buffered, 1, &format_id(identity));
            proto::uint(&mut buffered, 2, first.start_ms);
            proto::uint(&mut buffered, 3, duration_ms);
            proto::uint(&mut buffered, 4, u64::from(first.sequence));
            proto::uint(&mut buffered, 5, u64::from(last.sequence));
            let mut time_range = Vec::new();
            proto::uint(&mut time_range, 1, first.start_ms);
            proto::uint(&mut time_range, 2, duration_ms);
            proto::uint(&mut time_range, 3, 1000);
            proto::bytes(&mut buffered, 6, &time_range);
            proto::bytes(&mut out, 3, &buffered);
        }
    }
    out
}
fn numbered_url(url: &Url, number: u64) -> Url {
    let mut url = url.clone();
    // Match the public primary client's per-POST request counter. Keep every
    // signed parameter's decoded value; replace only an existing rn field.
    if url.query_pairs().any(|(key, _)| key == "rn") {
        let pairs: Vec<_> = url
            .query_pairs()
            .filter(|(key, _)| key != "rn")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        url.query_pairs_mut()
            .clear()
            .extend_pairs(pairs)
            .append_pair("rn", &number.to_string());
    } else {
        let query = url.query().map_or_else(
            || format!("rn={number}"),
            |query| format!("{query}&rn={number}"),
        );
        url.set_query(Some(&query));
    }
    url
}
#[allow(clippy::too_many_arguments)]
async fn run(
    downloader: &HttpDownloader,
    stream: &ResolvedStream,
    config: &SabrStream,
    mut url: Url,
    target: &Path,
    control: &Control,
    progress: &ProgressSink,
) -> Result<PathBuf> {
    let part = companion(target, ".part");
    let checkpoint = companion(target, ".checkpoint.json");
    let mut saved = if checkpoint.exists() {
        read_checkpoint(&checkpoint, stream, config).await?
    } else {
        if part.exists() {
            return Err(conflict("partial file has no SABR checkpoint"));
        }
        Checkpoint::new(stream, config)
    };
    let mut file = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&part)
        .await?;
    if file.metadata().await?.len() < saved.selected().bytes() {
        return Err(protocol("partial file is shorter than SABR checkpoint"));
    }
    verify_partial(&mut file, saved.selected(), control).await?;
    file.set_len(saved.selected().bytes()).await?;
    file.sync_data().await?;
    atomic_json(&checkpoint, &saved)?;
    let expected = saved.identity(config.selected_itag)?.content_length;
    let mut session = Session::default();
    let started = Instant::now();
    let initial = saved.selected().bytes();
    let mut stalls = 0;
    while saved.selected().bytes() < expected {
        control.check()?;
        let before = saved.selected().bytes();
        let mut response = None;
        let mut last_error = Error::new(ErrorCode::Network, "SABR request attempts exhausted");
        for attempt in 0..ATTEMPTS {
            if attempt > 0 {
                controlled(
                    control,
                    tokio::time::sleep(Duration::from_millis(250 * (1 << (attempt - 1)))),
                )
                .await?;
            }
            let body = request(
                &saved,
                config,
                &session,
                started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            );
            let request_url = numbered_url(&url, session.request_number);
            session.request_number = session
                .request_number
                .checked_add(1)
                .ok_or_else(|| protocol("SABR request counter overflow"))?;
            match fetch(downloader, &request_url, config, body, control)
                .await
                .and_then(|body| parse_response(&body, &saved, control))
            {
                Ok(parsed) => {
                    response = Some(parsed);
                    break;
                }
                Err(error) if error.code == ErrorCode::Network => last_error = error,
                Err(error) => return Err(error),
            }
        }
        let parsed = response.ok_or(last_error)?;
        let pending_protection = parsed.pending_protection;
        for segment in parsed.segments {
            control.check()?;
            commit(&mut file, &mut saved, segment, &checkpoint, control).await?;
            progress(Progress {
                bytes: saved.selected().bytes(),
                total: Some(expected),
                bytes_per_second: Some(
                    saved.selected().bytes().saturating_sub(initial) as f64
                        / started.elapsed().as_secs_f64().max(0.001),
                ),
            });
        }
        if let Some(cookie) = parsed.cookie {
            session.cookie = cookie;
        }
        apply_contexts(&mut session, parsed.context_actions)?;
        if let Some(redirect) = parsed.redirect {
            let next = Url::parse(&redirect).map_err(|_| protocol("invalid SABR redirect URL"))?;
            if !permitted(&next, downloader.allow_loopback) || session.redirects >= 5 {
                return Err(protocol("SABR redirect host or limit rejected"));
            }
            session.redirects += 1;
            url = next;
        }
        if saved.selected().bytes() == before {
            stalls += 1;
        } else {
            stalls = 0;
        }
        if stalls >= 3 {
            return Err(if pending_protection {
                unavailable("SABR protection pending without media progress")
            } else {
                Error::new(ErrorCode::Network, "SABR made no media progress")
            });
        }
        if parsed.backoff_ms > 0 {
            if parsed.backoff_ms > 30_000 {
                return Err(unavailable("SABR server requested excessive backoff"));
            }
            controlled(
                control,
                tokio::time::sleep(Duration::from_millis(parsed.backoff_ms)),
            )
            .await?;
        }
    }
    control.check()?;
    file.sync_all().await?;
    drop(file);
    atomic_json(&companion(target, ".complete.json"), &saved)?;
    tokio::fs::hard_link(&part, target).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            conflict("download target already exists")
        } else {
            error.into()
        }
    })?;
    tokio::fs::remove_file(&part).await?;
    tokio::fs::remove_file(&checkpoint).await?;
    progress(Progress {
        bytes: expected,
        total: Some(expected),
        bytes_per_second: None,
    });
    Ok(target.to_path_buf())
}
async fn fetch(
    downloader: &HttpDownloader,
    url: &Url,
    config: &SabrStream,
    body: Vec<u8>,
    control: &Control,
) -> Result<Vec<u8>> {
    let fetch = async {
        let response = controlled(
            control,
            downloader
                .client
                .post(url.clone())
                .timeout(Duration::from_secs(30))
                .header(header::CONTENT_TYPE, "application/x-protobuf")
                .header(header::ACCEPT, "application/vnd.yt-ump")
                .header(header::ACCEPT_ENCODING, "identity")
                .header(header::USER_AGENT, &config.user_agent)
                .body(body)
                .send(),
        )
        .await?
        .map_err(|_| Error::new(ErrorCode::Network, "SABR request failed"))?;
        let status = response.status();
        if !permitted(response.url(), downloader.allow_loopback) {
            return Err(protocol("SABR effective response host rejected"));
        }
        if matches!(
            status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::GONE
        ) {
            return Err(unavailable(
                "SABR URL expired or playback denied; resolve stream again",
            ));
        }
        if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            return Err(Error::new(
                ErrorCode::Network,
                format!("SABR server returned {}", status.as_u16()),
            ));
        }
        if status != StatusCode::OK {
            return Err(protocol("SABR requires HTTP 200"));
        }
        let mime = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|s| s.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        if mime != "application/vnd.yt-ump"
            || response
                .headers()
                .get(header::CONTENT_ENCODING)
                .is_some_and(|v| v != "identity")
            || response
                .content_length()
                .is_some_and(|n| n > MAX_RESPONSE as u64)
        {
            return Err(protocol("invalid or oversized SABR response"));
        }
        let mut chunks = response.bytes_stream();
        let mut out = Vec::new();
        while let Some(chunk) = controlled(control, chunks.next()).await? {
            let chunk =
                chunk.map_err(|_| Error::new(ErrorCode::Network, "SABR response interrupted"))?;
            if chunk.len() > MAX_RESPONSE - out.len() {
                return Err(protocol("SABR response size limit exceeded"));
            }
            out.extend_from_slice(&chunk);
        }
        if out.is_empty() {
            return Err(protocol("empty SABR response"));
        }
        Ok(out)
    };
    tokio::time::timeout(Duration::from_secs(35), fetch)
        .await
        .map_err(|_| Error::new(ErrorCode::Network, "SABR response timed out"))?
}
struct Segment {
    itag: u32,
    completed: Completed,
    data: Vec<u8>,
}
struct Pending {
    itag: u32,
    completed: Completed,
    data: Vec<u8>,
}
#[derive(Default)]
struct Response {
    segments: Vec<Segment>,
    cookie: Option<Vec<u8>>,
    pending_protection: bool,
    backoff_ms: u64,
    context_actions: Vec<ContextAction>,
    redirect: Option<String>,
}
fn u32_field(message: &Message<'_>, field: u32) -> Result<Option<u32>> {
    message
        .uint(field)?
        .map(|n| u32::try_from(n).map_err(|_| protocol("SABR metadata integer overflow")))
        .transpose()
}
fn verify_format(message: &Message<'_>, identity: &Identity) -> Result<()> {
    if message
        .uint(1)?
        .is_some_and(|n| n != u64::from(identity.itag))
        || message
            .uint(2)?
            .is_some_and(|n| n != identity.last_modified)
        || message
            .text(3)?
            .is_some_and(|s| s != identity.xtags.as_deref().unwrap_or(""))
    {
        return Err(conflict("SABR media format revision changed"));
    }
    Ok(())
}
fn parse_header(data: &[u8], saved: &Checkpoint) -> Result<(u32, Pending)> {
    let header = Message::parse(data)?;
    let nested = header.bytes(13)?.map(Message::parse).transpose()?;
    let itag = u32_field(&header, 3)?
        .or(nested
            .as_ref()
            .map(|m| u32_field(m, 1))
            .transpose()?
            .flatten())
        .ok_or_else(|| protocol("SABR media header lacks format ID"))?;
    let identity = saved.identity(itag)?;
    if header.text(2)?.is_some_and(|id| id != saved.media_id) {
        return Err(conflict("SABR media video identity changed"));
    }
    if header
        .uint(4)?
        .is_some_and(|lmt| lmt != identity.last_modified)
        || header
            .text(5)?
            .is_some_and(|s| s != identity.xtags.as_deref().unwrap_or(""))
    {
        return Err(conflict("SABR media format revision changed"));
    }
    if let Some(nested) = nested {
        verify_format(&nested, identity)?;
    }
    if header.uint(7)?.unwrap_or(0) != 0 {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "compressed SABR media is unsupported",
        ));
    }
    let length = header
        .uint(14)?
        .filter(|n| *n > 0 && *n <= MAX_SEGMENT)
        .ok_or_else(|| protocol("invalid SABR segment length"))?;
    if header.uint(17)?.unwrap_or(0) != 0 || header.uint(18)?.is_some_and(|n| n != length) {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "partial SABR segments are unsupported",
        ));
    }
    let initialization = match header.uint(8)?.unwrap_or(0) {
        0 => false,
        1 => true,
        _ => return Err(protocol("invalid SABR initialization flag")),
    };
    let offset = header
        .uint(6)?
        .or(initialization.then_some(0))
        .ok_or_else(|| protocol("SABR media header lacks absolute byte offset"))?;
    if offset
        .checked_add(length)
        .is_none_or(|n| n > identity.content_length)
    {
        return Err(protocol("SABR segment exceeds declared format length"));
    }
    let start_ms = header.uint(11)?.unwrap_or(0);
    let duration_ms = header.uint(12)?.unwrap_or(0);
    if duration_ms > MAX_DURATION
        || start_ms.saturating_add(duration_ms) > saved.duration_ms.saturating_add(1000)
        || (!initialization && duration_ms == 0)
    {
        return Err(protocol("invalid SABR segment timing"));
    }
    let header_id = u32_field(&header, 1)?
        .filter(|n| *n <= 255)
        .ok_or_else(|| protocol("SABR media header ID exceeds supported uint8 range"))?;
    let sequence = u32_field(&header, 9)?
        .or(initialization.then_some(0))
        .ok_or_else(|| protocol("SABR media header lacks sequence"))?;
    Ok((
        header_id,
        Pending {
            itag,
            completed: Completed {
                sequence,
                initialization,
                offset,
                length,
                start_ms,
                duration_ms,
                sha256: String::new(),
            },
            data: Vec::new(),
        },
    ))
}
fn parse_response(data: &[u8], saved: &Checkpoint, control: &Control) -> Result<Response> {
    let mut response = Response::default();
    let mut pending = BTreeMap::new();
    for (kind, data) in proto::parts(data)? {
        control.check()?;
        match kind {
            20 => {
                let (id, segment) = parse_header(data, saved)?;
                if pending.len() >= 64 || pending.insert(id, segment).is_some() {
                    return Err(protocol("duplicate or excessive SABR media headers"));
                }
            }
            21 | 22 => {
                // The pinned clear-VOD implementation uses one raw uint8
                // header ID here, unlike the UMP part framing integers.
                let id = u32::from(
                    *data
                        .first()
                        .ok_or_else(|| protocol("empty SABR media part"))?,
                );
                let offset = 1;
                let segment = pending
                    .get_mut(&id)
                    .ok_or_else(|| protocol("SABR media references unknown header"))?;
                if kind == 21 {
                    if data.len() == offset
                        || (data.len() - offset) as u64
                            > segment
                                .completed
                                .length
                                .saturating_sub(segment.data.len() as u64)
                    {
                        return Err(protocol("SABR media exceeds segment length"));
                    }
                    segment.data.extend_from_slice(&data[offset..]);
                } else {
                    if offset != data.len() {
                        return Err(protocol("invalid SABR media end marker"));
                    }
                    let mut segment = pending.remove(&id).expect("checked header");
                    if segment.data.len() as u64 != segment.completed.length {
                        return Err(Error::new(
                            ErrorCode::Network,
                            "incomplete SABR media segment",
                        ));
                    }
                    segment.completed.sha256 = format!("{:x}", Sha256::digest(&segment.data));
                    response.segments.push(Segment {
                        itag: segment.itag,
                        completed: segment.completed,
                        data: segment.data,
                    });
                }
            }
            35 => {
                let policy = Message::parse(data)?;
                if policy.text(8)?.is_some_and(|id| id != saved.media_id) {
                    return Err(conflict("SABR policy video identity changed"));
                }
                response.backoff_ms = policy.uint(4)?.unwrap_or(0);
                if let Some(cookie) = policy.bytes(7)? {
                    if cookie.len() > 65536 {
                        return Err(protocol("SABR cookie size limit exceeded"));
                    }
                    response.cookie = Some(cookie.to_vec());
                }
            }
            42 => {
                let metadata = Message::parse(data)?;
                if metadata.text(1)?.is_some_and(|id| id != saved.media_id) {
                    return Err(conflict("SABR metadata video identity changed"));
                }
                let format = Message::parse(
                    metadata
                        .bytes(2)?
                        .ok_or_else(|| protocol("SABR metadata lacks format ID"))?,
                )?;
                let identity = saved.identity(
                    u32_field(&format, 1)?.ok_or_else(|| protocol("SABR metadata lacks itag"))?,
                )?;
                verify_format(&format, identity)?;
            }
            43 => {
                let redirect = Message::parse(data)?
                    .text(1)?
                    .filter(|s| s.len() <= 65536)
                    .ok_or_else(|| protocol("invalid SABR redirect"))?
                    .to_owned();
                if response.redirect.replace(redirect).is_some() {
                    return Err(protocol("multiple SABR redirects in one response"));
                }
            }
            44 => return Err(unavailable("SABR server rejected playback")),
            57 => {
                let update = Message::parse(data)?;
                let kind =
                    u32_field(&update, 1)?.ok_or_else(|| protocol("SABR context lacks type"))?;
                let value = update
                    .bytes(3)?
                    .ok_or_else(|| protocol("SABR context lacks value"))?;
                if value.len() > 65536 {
                    return Err(protocol("SABR context size limit exceeded"));
                }
                response.context_actions.push(ContextAction::Update(
                    kind,
                    Context {
                        value: value.to_vec(),
                        active: update.uint(4)?.unwrap_or(0) != 0,
                    },
                    update.uint(5)?.unwrap_or(0) == 2,
                ));
            }
            58 => match Message::parse(data)?.uint(1)?.unwrap_or(0) {
                0 | 1 => {}
                2 => response.pending_protection = true,
                3 => {
                    return Err(unavailable(
                        "SABR playback requires a valid video-bound proof",
                    ));
                }
                _ => return Err(protocol("unknown SABR stream protection status")),
            },
            59 => {
                let policy = Message::parse(data)?;
                response
                    .context_actions
                    .extend(policy.uints(1)?.into_iter().map(ContextAction::Start));
                response
                    .context_actions
                    .extend(policy.uints(2)?.into_iter().map(ContextAction::Stop));
                response
                    .context_actions
                    .extend(policy.uints(3)?.into_iter().map(ContextAction::Discard));
            }
            12 => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "encrypted SABR media is unsupported",
                ));
            }
            31 | 45 => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "live or server-seek SABR is unsupported",
                ));
            }
            46 => return Err(unavailable("SABR requested fresh player metadata")),
            _ => {}
        }
        if response.context_actions.len() > 256 {
            return Err(protocol("SABR context policy limit exceeded"));
        }
    }
    if !pending.is_empty() {
        return Err(Error::new(
            ErrorCode::Network,
            "incomplete SABR media response",
        ));
    }
    Ok(response)
}
async fn commit(
    file: &mut tokio::fs::File,
    saved: &mut Checkpoint,
    segment: Segment,
    path: &Path,
    control: &Control,
) -> Result<()> {
    let track = saved.track(segment.itag);
    let completed = &segment.completed;
    if completed.offset < track.bytes() {
        let old = track
            .segments
            .iter()
            .find(|old| old.offset == completed.offset)
            .ok_or_else(|| conflict("SABR repeated segment boundary changed"))?;
        if old.sequence != completed.sequence
            || old.initialization != completed.initialization
            || old.length != completed.length
            || old.start_ms != completed.start_ms
            || old.duration_ms != completed.duration_ms
            || old.sha256 != completed.sha256
        {
            return Err(conflict("SABR repeated segment content changed"));
        }
        if segment.itag == saved.selected_itag {
            file.seek(std::io::SeekFrom::Start(completed.offset))
                .await?;
            let mut actual = vec![0; segment.data.len()];
            file.read_exact(&mut actual).await?;
            if actual != segment.data {
                return Err(conflict("SABR partial file content changed"));
            }
        }
        return Ok(());
    }
    if completed.offset != track.bytes() {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "gapped SABR media is unsupported",
        ));
    }
    if completed.initialization != track.segments.is_empty()
        || (track.segments.is_empty() && completed.sequence != 0)
        || track.segments.last().is_some_and(|old| {
            old.sequence.checked_add(1) != Some(completed.sequence)
                || completed.start_ms < old.start_ms
        })
        || track.segments.len() >= MAX_SEGMENTS
    {
        return Err(protocol("SABR segment sequence changed or exceeded limit"));
    }
    if completed.start_ms.abs_diff(track.end_ms()) > 2 {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "gapped SABR segment timing is unsupported",
        ));
    }
    if segment.itag == saved.selected_itag {
        control.check()?;
        file.seek(std::io::SeekFrom::Start(completed.offset))
            .await?;
        file.write_all(&segment.data).await?;
        file.sync_data().await?;
    }
    saved
        .track_mut(segment.itag)
        .segments
        .push(segment.completed);
    atomic_json(path, saved)?;
    Ok(())
}

#[cfg(test)]
#[path = "sabr_tests.rs"]
mod tests;
