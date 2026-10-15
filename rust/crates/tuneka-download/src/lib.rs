//! Streaming HTTP downloads with durable, URL-free resume checkpoints.
//! Pausing and transient failures retain the partial download; cancellation deletes it.
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode, Url, header};
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;
use tuneka_core::{
    Control, Downloader, Error, ErrorCode, Progress, ProgressSink, ResolvedStream, Result,
    files::{FileLock, atomic_json},
};

const ATTEMPTS: usize = 4;
mod sabr;
mod segmented;

pub struct HttpDownloader {
    client: Client,
    allow_loopback: bool,
}

fn permitted(url: &Url, allow_loopback: bool) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let host = url.host_str().unwrap_or("");
    (url.scheme() == "https"
        && (host == "googlevideo.com" || host.ends_with(".googlevideo.com"))
        && url.port_or_known_default() == Some(443))
        || (allow_loopback && url.scheme() == "http" && matches!(host, "127.0.0.1" | "[::1]"))
}

impl HttpDownloader {
    pub fn new() -> Result<Self> {
        Self::build(false)
    }
    fn build(allow_loopback: bool) -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60 * 60))
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 5 || !permitted(attempt.url(), allow_loopback) {
                    attempt.error("media redirect rejected")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|_| Error::new(ErrorCode::Network, "cannot initialize HTTP client"))?;
        Ok(Self {
            client,
            allow_loopback,
        })
    }
    #[cfg(test)]
    fn for_local_tests() -> Self {
        Self::build(true).unwrap()
    }
}

#[derive(Serialize, Deserialize)]
struct Checkpoint {
    version: u8,
    media_id: String,
    format_id: String,
    declared_length: Option<u64>,
    total: Option<u64>,
    bytes: u64,
}

fn direct_checkpoint(bytes: &[u8]) -> Result<Checkpoint> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    if value.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err(Error::new(
            ErrorCode::Conflict,
            "checkpoint download mode changed",
        ));
    }
    Ok(serde_json::from_value(value)?)
}

fn companion(target: &Path, suffix: &str) -> PathBuf {
    let mut path = target.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}

async fn controlled<T>(control: &Control, future: impl Future<Output = T>) -> Result<T> {
    tokio::pin!(future);
    loop {
        control.check()?;
        tokio::select! {
            result = &mut future => { control.check()?; return Ok(result); }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    }
}

fn range(value: &str) -> Option<(u64, u64, u64)> {
    let (bounds, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = bounds.split_once('-')?;
    let (start, end, total) = (start.parse().ok()?, end.parse().ok()?, total.parse().ok()?);
    if start > end || end >= total {
        return None;
    }
    Some((start, end, total))
}

#[async_trait]
impl Downloader for HttpDownloader {
    async fn download(
        &self,
        stream: ResolvedStream,
        target: PathBuf,
        control: Control,
        progress: ProgressSink,
    ) -> Result<PathBuf> {
        if stream.sabr.is_some() {
            if stream.segmented.is_some() {
                return Err(Error::new(
                    ErrorCode::InvalidInput,
                    "ambiguous stream transport",
                ));
            }
            return sabr::download(self, stream, target, control, progress).await;
        }
        if stream.segmented.is_some() {
            return segmented::download(self, stream, target, control, progress).await;
        }
        let url = Url::parse(&stream.url)
            .map_err(|_| Error::new(ErrorCode::InvalidInput, "invalid media URL"))?;
        if !permitted(&url, self.allow_loopback) {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "media URL must use HTTPS googlevideo host",
            ));
        }
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let _lock = FileLock::acquire(&companion(&target, ".lock"))?;
        if target.exists() {
            control.check()?;
            let receipt_path = companion(&target, ".complete.json");
            if let Ok(bytes) = tokio::fs::read(receipt_path).await {
                let receipt = direct_checkpoint(&bytes)?;
                if receipt.version == 1
                    && receipt.media_id == stream.media_id
                    && receipt.format_id == stream.format.id
                    && receipt.declared_length == stream.format.content_length
                    && receipt.total == Some(receipt.bytes)
                    && tokio::fs::metadata(&target).await?.len() == receipt.bytes
                {
                    return Ok(target);
                }
            }
            return Err(Error::new(
                ErrorCode::Conflict,
                "download target has no matching completion receipt",
            ));
        }
        let part = companion(&target, ".part");
        let checkpoint_path = companion(&target, ".checkpoint.json");
        let result = self
            .run(
                &stream,
                &url,
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
            for path in [&part, &checkpoint_path] {
                match tokio::fs::remove_file(path).await {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        result
    }
}

impl HttpDownloader {
    #[allow(clippy::too_many_arguments)]
    async fn run(
        &self,
        stream: &ResolvedStream,
        url: &Url,
        target: &Path,
        part: &Path,
        checkpoint_path: &Path,
        control: &Control,
        progress: &ProgressSink,
    ) -> Result<PathBuf> {
        control.check()?;
        let mut checkpoint = if checkpoint_path.exists() {
            let saved = direct_checkpoint(&tokio::fs::read(checkpoint_path).await?)?;
            if saved.version != 1
                || saved.media_id != stream.media_id
                || saved.format_id != stream.format.id
                || saved.declared_length != stream.format.content_length
            {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "resume stream identity changed",
                ));
            }
            if saved.total.is_some_and(|total| saved.bytes > total)
                || saved
                    .declared_length
                    .zip(saved.total)
                    .is_some_and(|(a, b)| a != b)
            {
                return Err(Error::new(ErrorCode::Protocol, "invalid resume checkpoint"));
            }
            saved
        } else {
            if part.exists() {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "partial file has no checkpoint",
                ));
            }
            Checkpoint {
                version: 1,
                media_id: stream.media_id.clone(),
                format_id: stream.format.id.clone(),
                declared_length: stream.format.content_length,
                total: stream.format.content_length,
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
        if file.metadata().await?.len() < checkpoint.bytes {
            return Err(Error::new(
                ErrorCode::Protocol,
                "partial file is shorter than checkpoint",
            ));
        }
        file.set_len(checkpoint.bytes).await?;
        use tokio::io::AsyncSeekExt;
        file.seek(std::io::SeekFrom::Start(checkpoint.bytes))
            .await?;
        file.sync_data().await?;
        atomic_json(checkpoint_path, &checkpoint)?;
        let started = Instant::now();
        let initial = checkpoint.bytes;
        let mut last_error = Error::new(ErrorCode::Network, "download attempts exhausted");
        let mut attempt = 0;
        while attempt < ATTEMPTS {
            control.check()?;
            if checkpoint.total == Some(checkpoint.bytes) {
                break;
            }
            if attempt > 0 {
                controlled(
                    control,
                    tokio::time::sleep(Duration::from_millis(250 * (1 << (attempt - 1)))),
                )
                .await?;
            }
            attempt += 1;
            let offset = checkpoint.bytes;
            // YouTube CDN requires finite ranges even for a fresh transfer.
            let requested_end = checkpoint
                .total
                .map(|n| n.saturating_sub(1))
                .unwrap_or(u64::MAX)
                .min(offset.saturating_add(1024 * 1024 - 1));
            let request = self
                .client
                .get(url.clone())
                .header(header::ACCEPT_ENCODING, "identity")
                .header(header::RANGE, format!("bytes={offset}-{requested_end}"));
            let response = match controlled(control, request.send()).await? {
                Ok(response) => response,
                Err(_) => {
                    last_error = Error::new(ErrorCode::Network, "media request failed");
                    continue;
                }
            };
            let status = response.status();
            if matches!(
                status,
                StatusCode::FORBIDDEN | StatusCode::GONE | StatusCode::UNAUTHORIZED
            ) {
                return Err(Error::new(
                    ErrorCode::Unavailable,
                    format!(
                        "media server returned {}; URL expired or access denied; resolve stream again",
                        status.as_u16()
                    ),
                ));
            }
            if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                last_error = Error::new(
                    ErrorCode::Network,
                    format!("media server returned {}", status.as_u16()),
                );
                continue;
            }
            if response
                .headers()
                .get(header::CONTENT_ENCODING)
                .is_some_and(|v| v != "identity")
            {
                return Err(Error::new(
                    ErrorCode::Protocol,
                    "encoded media response is unsupported",
                ));
            }
            let response_end = if status == StatusCode::PARTIAL_CONTENT {
                let (start, end, total) = response
                    .headers()
                    .get(header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(range)
                    .ok_or_else(|| Error::new(ErrorCode::Protocol, "invalid Content-Range"))?;
                if start != offset
                    || end > requested_end
                    || checkpoint.total.is_some_and(|old| old != total)
                    || response
                        .content_length()
                        .is_some_and(|len| len != end - start + 1)
                {
                    return Err(Error::new(
                        ErrorCode::Protocol,
                        "range response does not match checkpoint",
                    ));
                }
                checkpoint.total = Some(total);
                Some(end + 1)
            } else if status == StatusCode::OK {
                let total = response.content_length();
                if checkpoint.total.zip(total).is_some_and(|(a, b)| a != b) {
                    return Err(Error::new(
                        ErrorCode::Protocol,
                        "media content length changed",
                    ));
                }
                if offset > 0 {
                    // Commit the restart checkpoint before truncating: a crash
                    // can then discard old trailing bytes on the next resume.
                    checkpoint.bytes = 0;
                    atomic_json(checkpoint_path, &checkpoint)?;
                    file.set_len(0).await?;
                    file.seek(std::io::SeekFrom::Start(0)).await?;
                }
                checkpoint.total = checkpoint.total.or(total);
                checkpoint.total
            } else {
                return Err(Error::new(
                    ErrorCode::Protocol,
                    format!("unexpected media status {}", status.as_u16()),
                ));
            };
            file.sync_data().await?;
            atomic_json(checkpoint_path, &checkpoint)?;
            let mut body = response.bytes_stream();
            let mut interrupted = false;
            while let Some(chunk) = controlled(control, body.next()).await? {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(_) => {
                        interrupted = true;
                        last_error = Error::new(ErrorCode::Network, "media response interrupted");
                        break;
                    }
                };
                let next = checkpoint
                    .bytes
                    .checked_add(chunk.len() as u64)
                    .ok_or_else(|| Error::new(ErrorCode::Protocol, "media size overflow"))?;
                if response_end.is_some_and(|end| next > end) {
                    return Err(Error::new(
                        ErrorCode::Protocol,
                        "media response exceeds expected length",
                    ));
                }
                file.write_all(&chunk).await?;
                file.sync_data().await?;
                checkpoint.bytes = next;
                atomic_json(checkpoint_path, &checkpoint)?;
                progress(Progress {
                    bytes: next,
                    total: checkpoint.total,
                    bytes_per_second: Some(
                        next.saturating_sub(initial) as f64
                            / started.elapsed().as_secs_f64().max(0.001),
                    ),
                });
            }
            if interrupted {
                continue;
            }
            if response_end.is_some_and(|end| checkpoint.bytes != end) {
                last_error = Error::new(ErrorCode::Network, "truncated media response");
                continue;
            }
            if checkpoint.total.is_none() {
                checkpoint.total = Some(checkpoint.bytes);
            }
            if checkpoint.total == Some(checkpoint.bytes) {
                break;
            }
            // A successful finite chunk is progress, not a consumed retry.
            attempt = 0;
        }
        control.check()?;
        if checkpoint.total != Some(checkpoint.bytes) {
            return Err(last_error);
        }
        file.sync_all().await?;
        drop(file);
        // Hard-link publication is atomic and fails if an external writer created the target.
        atomic_json(&companion(target, ".complete.json"), &checkpoint)?;
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
            bytes: checkpoint.bytes,
            total: checkpoint.total,
            bytes_per_second: None,
        });
        Ok(target.to_path_buf())
    }
}

#[cfg(test)]
mod tests;
