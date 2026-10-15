//! Finite VOD concatenation. Only completed segments become durable resume state.
use super::{
    ATTEMPTS, FileLock, HttpDownloader, atomic_json, companion, controlled, permitted, range,
};
use futures_util::StreamExt;
use reqwest::{StatusCode, Url, header};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tuneka_core::{
    Control, Error, ErrorCode, MediaSegment, Progress, ProgressSink, ResolvedStream, Result,
    SegmentedStream,
};

const MAX_SEGMENT: u64 = 128 * 1024 * 1024;
const MAX_SEGMENTS: usize = 20_000;
#[derive(Serialize, Deserialize)]
struct Checkpoint {
    version: u8,
    media_id: String,
    format_id: String,
    declared_length: Option<u64>,
    identity: String,
    segment_count: usize,
    next_segment: usize,
    bytes: u64,
}

fn invalid(message: &str) -> Error {
    Error::new(ErrorCode::InvalidInput, message)
}
fn protocol(message: &str) -> Error {
    Error::new(ErrorCode::Protocol, message)
}

/// Validate the entire plan before creating a directory, lock or network request.
fn validate(plan: &SegmentedStream, allow_loopback: bool) -> Result<(Vec<Url>, Option<u64>)> {
    if plan.identity.len() != 64 || !plan.identity.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("segmented stream identity must be SHA-256"));
    }
    if plan.segments.is_empty() || plan.segments.len() > MAX_SEGMENTS {
        return Err(invalid("segmented stream must contain 1 to 20000 segments"));
    }
    let mut urls = Vec::with_capacity(plan.segments.len());
    let mut total = Some(0_u64);
    for segment in &plan.segments {
        let url = Url::parse(&segment.url).map_err(|_| invalid("invalid segment URL"))?;
        if !permitted(&url, allow_loopback) {
            return Err(invalid("segment URL must use HTTPS googlevideo host"));
        }
        if let Some((start, end)) = segment.byte_range {
            let length = end
                .checked_sub(start)
                .and_then(|n| n.checked_add(1))
                .filter(|n| *n <= MAX_SEGMENT)
                .ok_or_else(|| invalid("invalid or oversized segment byte range"))?;
            total = total.map(|sum| sum + length);
        } else {
            total = None;
        }
        urls.push(url);
    }
    Ok((urls, total))
}

fn read_checkpoint(
    bytes: &[u8],
    stream: &ResolvedStream,
    plan: &SegmentedStream,
) -> Result<Checkpoint> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    if value.get("version").and_then(serde_json::Value::as_u64) != Some(2) {
        return Err(Error::new(
            ErrorCode::Conflict,
            "checkpoint download mode changed",
        ));
    }
    let saved: Checkpoint = serde_json::from_value(value)?;
    if saved.media_id != stream.media_id
        || saved.format_id != stream.format.id
        || saved.declared_length != stream.format.content_length
        || saved.identity != plan.identity
        || saved.segment_count != plan.segments.len()
    {
        return Err(Error::new(
            ErrorCode::Conflict,
            "segmented resume identity changed",
        ));
    }
    if saved.next_segment > plan.segments.len()
        || saved.bytes > saved.next_segment as u64 * MAX_SEGMENT
        || saved.bytes < saved.next_segment as u64
    {
        return Err(protocol("invalid segmented checkpoint boundary"));
    }
    let mut exact = Some(0_u64);
    for segment in plan.segments.iter().take(saved.next_segment) {
        exact = match (exact, segment.byte_range) {
            (Some(sum), Some((start, end))) => Some(sum + (end - start + 1)),
            _ => None,
        };
    }
    if exact.is_some_and(|n| n != saved.bytes)
        || saved.declared_length.is_some_and(|n| saved.bytes > n)
    {
        return Err(protocol("invalid segmented checkpoint byte count"));
    }
    Ok(saved)
}

pub(super) async fn download(
    downloader: &HttpDownloader,
    stream: ResolvedStream,
    target: PathBuf,
    control: Control,
    progress: ProgressSink,
) -> Result<PathBuf> {
    let plan = stream.segmented.as_ref().expect("segmented dispatch");
    let (urls, ranged_total) = validate(plan, downloader.allow_loopback)?;
    if ranged_total
        .zip(stream.format.content_length)
        .is_some_and(|(a, b)| a != b)
    {
        return Err(invalid(
            "segment ranges disagree with declared stream length",
        ));
    }
    let total = stream.format.content_length.or(ranged_total);
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let _lock = FileLock::acquire(&companion(&target, ".lock"))?;
    if let Err(error) = control.check() {
        // Cancellation also cleans a stopped, previously paused transfer.
        if error.code == ErrorCode::Cancelled {
            cleanup(&target).await?;
        }
        return Err(error);
    }
    if target.exists() {
        let saved = read_checkpoint(
            &tokio::fs::read(companion(&target, ".complete.json"))
                .await
                .map_err(|_| {
                    Error::new(
                        ErrorCode::Conflict,
                        "download target has no completion receipt",
                    )
                })?,
            &stream,
            plan,
        )?;
        if saved.next_segment == plan.segments.len()
            && saved.bytes == tokio::fs::metadata(&target).await?.len()
            && total.is_none_or(|n| saved.bytes == n)
        {
            return Ok(target);
        }
        return Err(Error::new(
            ErrorCode::Conflict,
            "download target does not match completion receipt",
        ));
    }
    let part = companion(&target, ".part");
    let checkpoint_path = companion(&target, ".checkpoint.json");
    let result = run(
        downloader,
        &stream,
        plan,
        &urls,
        total,
        &target,
        &part,
        &checkpoint_path,
        &control,
        &progress,
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
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run(
    downloader: &HttpDownloader,
    stream: &ResolvedStream,
    plan: &SegmentedStream,
    urls: &[Url],
    total: Option<u64>,
    target: &Path,
    part: &Path,
    checkpoint_path: &Path,
    control: &Control,
    progress: &ProgressSink,
) -> Result<PathBuf> {
    let mut saved = if checkpoint_path.exists() {
        read_checkpoint(&tokio::fs::read(checkpoint_path).await?, stream, plan)?
    } else {
        if part.exists() {
            return Err(Error::new(
                ErrorCode::Conflict,
                "partial file has no segmented checkpoint",
            ));
        }
        Checkpoint {
            version: 2,
            media_id: stream.media_id.clone(),
            format_id: stream.format.id.clone(),
            declared_length: stream.format.content_length,
            identity: plan.identity.clone(),
            segment_count: plan.segments.len(),
            next_segment: 0,
            bytes: 0,
        }
    };
    let mut file = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(part)
        .await?;
    if file.metadata().await?.len() < saved.bytes {
        return Err(protocol(
            "partial file is shorter than segmented checkpoint",
        ));
    }
    rollback(&mut file, saved.bytes).await?;
    atomic_json(checkpoint_path, &saved)?;
    let started = Instant::now();
    let initial = saved.bytes;
    while saved.next_segment < plan.segments.len() {
        control.check()?;
        let index = saved.next_segment;
        let mut error = Error::new(ErrorCode::Network, "segment attempts exhausted");
        let mut committed = None;
        for attempt in 0..ATTEMPTS {
            rollback(&mut file, saved.bytes).await?;
            if attempt > 0 {
                controlled(
                    control,
                    tokio::time::sleep(Duration::from_millis(250 * (1 << (attempt - 1)))),
                )
                .await?;
            }
            let transfer = tokio::time::timeout(
                Duration::from_secs(120),
                transfer(
                    downloader,
                    &plan.segments[index],
                    &urls[index],
                    &mut file,
                    saved.bytes,
                    total,
                    control,
                    progress,
                    started,
                    initial,
                ),
            )
            .await;
            match transfer {
                Ok(Ok(length)) => {
                    committed = Some(length);
                    break;
                }
                outcome => {
                    rollback(&mut file, saved.bytes).await?;
                    error = match outcome {
                        Ok(Err(error)) => error,
                        _ => Error::new(ErrorCode::Network, "segment transfer timed out"),
                    };
                    if error.code != ErrorCode::Network {
                        return Err(error);
                    }
                }
            }
        }
        let length = committed.ok_or(error)?;
        file.sync_data().await?;
        saved.bytes = saved
            .bytes
            .checked_add(length)
            .ok_or_else(|| protocol("segmented byte count overflow"))?;
        saved.next_segment += 1;
        atomic_json(checkpoint_path, &saved)?;
        progress(Progress {
            bytes: saved.bytes,
            total,
            bytes_per_second: Some(
                saved.bytes.saturating_sub(initial) as f64
                    / started.elapsed().as_secs_f64().max(0.001),
            ),
        });
    }
    control.check()?;
    if total.is_some_and(|n| n != saved.bytes) {
        return Err(protocol("segmented stream length does not match plan"));
    }
    file.sync_all().await?;
    drop(file);
    atomic_json(&companion(target, ".complete.json"), &saved)?;
    tokio::fs::hard_link(part, target).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            Error::new(ErrorCode::Conflict, "download target already exists")
        } else {
            e.into()
        }
    })?;
    tokio::fs::remove_file(part).await?;
    tokio::fs::remove_file(checkpoint_path).await?;
    progress(Progress {
        bytes: saved.bytes,
        total: Some(saved.bytes),
        bytes_per_second: None,
    });
    Ok(target.to_path_buf())
}

async fn rollback(file: &mut tokio::fs::File, offset: u64) -> Result<()> {
    file.set_len(offset).await?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    file.sync_data().await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn transfer(
    downloader: &HttpDownloader,
    segment: &MediaSegment,
    url: &Url,
    file: &mut tokio::fs::File,
    offset: u64,
    total: Option<u64>,
    control: &Control,
    progress: &ProgressSink,
    started: Instant,
    initial: u64,
) -> Result<u64> {
    let mut request = downloader
        .client
        .get(url.clone())
        .header(header::ACCEPT_ENCODING, "identity");
    if let Some((start, end)) = segment.byte_range {
        request = request.header(header::RANGE, format!("bytes={start}-{end}"));
    }
    let response = controlled(control, request.send())
        .await?
        .map_err(|_| Error::new(ErrorCode::Network, "segment request failed"))?;
    let status = response.status();
    if matches!(
        status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::GONE
    ) {
        return Err(Error::new(
            ErrorCode::Unavailable,
            format!(
                "segment server returned {}; resolve stream again",
                status.as_u16()
            ),
        ));
    }
    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        return Err(Error::new(
            ErrorCode::Network,
            format!("segment server returned {}", status.as_u16()),
        ));
    }
    if response
        .headers()
        .get(header::CONTENT_ENCODING)
        .is_some_and(|v| v != "identity")
    {
        return Err(protocol("encoded segment response is unsupported"));
    }
    let expected = if let Some((start, end)) = segment.byte_range {
        if status != StatusCode::PARTIAL_CONTENT {
            return Err(protocol("segment byte range requires HTTP 206"));
        }
        let parsed = response
            .headers()
            .get(header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(range)
            .ok_or_else(|| protocol("invalid segment Content-Range"))?;
        if parsed.0 != start || parsed.1 != end {
            return Err(protocol("segment Content-Range mismatch"));
        }
        let length = end - start + 1;
        if response.content_length().is_some_and(|n| n != length) {
            return Err(protocol("segment range Content-Length mismatch"));
        }
        Some(length)
    } else {
        if status != StatusCode::OK {
            return Err(protocol("full segment requires HTTP 200"));
        }
        if response.headers().contains_key(header::CONTENT_RANGE) {
            return Err(protocol("unexpected Content-Range on full segment"));
        }
        response.content_length()
    };
    if expected.is_some_and(|n| n == 0 || n > MAX_SEGMENT) {
        return Err(protocol("empty or oversized media segment"));
    }
    let mut body = response.bytes_stream();
    let mut bytes = 0_u64;
    while let Some(chunk) = controlled(control, body.next()).await? {
        let chunk =
            chunk.map_err(|_| Error::new(ErrorCode::Network, "segment response interrupted"))?;
        bytes = bytes
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| protocol("segment byte count overflow"))?;
        if bytes > MAX_SEGMENT
            || expected.is_some_and(|n| bytes > n)
            || total.is_some_and(|n| offset.saturating_add(bytes) > n)
        {
            return Err(protocol("segment response exceeds expected length"));
        }
        file.write_all(&chunk).await?;
        progress(Progress {
            bytes: offset + bytes,
            total,
            bytes_per_second: Some(
                (offset + bytes).saturating_sub(initial) as f64
                    / started.elapsed().as_secs_f64().max(0.001),
            ),
        });
    }
    if bytes == 0 {
        return Err(protocol("empty media segment"));
    }
    if expected.is_some_and(|n| bytes != n) {
        return Err(Error::new(ErrorCode::Network, "truncated media segment"));
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "segmented_tests.rs"]
mod tests;
