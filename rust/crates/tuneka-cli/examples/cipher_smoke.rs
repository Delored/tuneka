//! Explicit captured-response live check. Requires private, unexpired MWEB
//! player JSON and its matching public player JS in environment-selected files.
//! The production player transform, downloader, engine and FFmpeg path run here.
#[path = "support/transport_smoke.rs"]
mod support;

use async_trait::async_trait;
use std::{
    io::Read,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tuneka_core::*;
use tuneka_youtube::Youtube;

struct CapturedCipher {
    youtube: Youtube,
    media_id: String,
    raw_format: serde_json::Value,
    player: Arc<String>,
    title: String,
    duration: Option<f64>,
    expires: u64,
}

impl CapturedCipher {
    async fn stream(&self) -> Result<ResolvedStream> {
        if self.expires <= now_unix().saturating_add(300) {
            return Err(Error::new(
                ErrorCode::Unavailable,
                "captured cipher URL has expired",
            ));
        }
        self.youtube
            .resolve_player_format(&self.media_id, &self.raw_format, self.player.clone())
            .await
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn read_bounded(path: PathBuf) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(10 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(Error::new(
            ErrorCode::Protocol,
            "captured input exceeds size limit",
        ));
    }
    Ok(bytes)
}

#[async_trait]
impl MetadataProvider for CapturedCipher {
    async fn inspect(&self, url: &str) -> Result<MediaInfo> {
        if url != format!("https://www.youtube.com/watch?v={}", self.media_id) {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "video ID does not match capture",
            ));
        }
        let stream = self.stream().await?;
        Ok(MediaInfo {
            id: self.media_id.clone(),
            title: self.title.clone(),
            duration: self.duration,
            chapters: vec![],
            has_chapters: false,
            thumbnails: vec![],
            formats: vec![stream.format],
            warnings: vec![],
        })
    }

    async fn playlist_page(&self, _url: &str, _continuation: Option<&str>) -> Result<PlaylistPage> {
        Err(Error::new(
            ErrorCode::Unsupported,
            "captured check has no playlist",
        ))
    }

    async fn resolve_stream(&self, media_id: &str, format_id: &str) -> Result<ResolvedStream> {
        if media_id != self.media_id {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "video ID does not match capture",
            ));
        }
        let stream = self.stream().await?;
        if stream.format.id != format_id {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "format does not match capture",
            ));
        }
        Ok(stream)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let response_path = std::env::var_os("TUNEKA_CIPHER_RESPONSE")
        .map(PathBuf::from)
        .ok_or_else(|| Error::new(ErrorCode::InvalidInput, "cipher response path required"))?;
    let player_path = std::env::var_os("TUNEKA_CIPHER_PLAYER")
        .map(PathBuf::from)
        .ok_or_else(|| Error::new(ErrorCode::InvalidInput, "matching player path required"))?;
    let response_bytes = read_bounded(response_path)?;
    let player_bytes = read_bounded(player_path)?;
    let response: serde_json::Value = serde_json::from_slice(&response_bytes)
        .map_err(|_| Error::new(ErrorCode::Protocol, "invalid captured response JSON"))?;
    let media_id = response["videoDetails"]["videoId"]
        .as_str()
        .filter(|id| {
            id.len() == 11
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        })
        .ok_or_else(|| Error::new(ErrorCode::Protocol, "capture lacks valid video ID"))?
        .to_owned();
    let raw = response["streamingData"]["formats"]
        .as_array()
        .ok_or_else(|| Error::new(ErrorCode::Unsupported, "no progressive formats"))?
        .iter()
        .filter(|format| {
            format["itag"].as_u64() == Some(18)
                && format["signatureCipher"].as_str().is_some()
                && format["url"].as_str().is_none()
                && format["drmFamilies"]
                    .as_array()
                    .is_none_or(|v| v.is_empty())
        })
        .collect::<Vec<_>>();
    if raw.len() != 1 {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "ambiguous cipher format",
        ));
    }
    let cipher = raw[0]["signatureCipher"]
        .as_str()
        .filter(|cipher| cipher.len() <= 65536)
        .ok_or_else(|| Error::new(ErrorCode::Protocol, "invalid cipher field"))?;
    let fields = url::form_urlencoded::parse(cipher.as_bytes()).collect::<Vec<_>>();
    let signature = fields
        .iter()
        .filter(|(key, _)| key == "s")
        .collect::<Vec<_>>();
    let nested = fields
        .iter()
        .filter(|(key, _)| key == "url")
        .collect::<Vec<_>>();
    if signature.len() != 1 || signature[0].1.is_empty() || nested.len() != 1 {
        return Err(Error::new(
            ErrorCode::Protocol,
            "cipher challenge is incomplete",
        ));
    }
    let nested = url::Url::parse(&nested[0].1)
        .map_err(|_| Error::new(ErrorCode::Protocol, "invalid nested cipher URL"))?;
    let n = nested
        .query_pairs()
        .filter(|(key, value)| key == "n" && !value.is_empty())
        .count();
    let expires = nested
        .query_pairs()
        .filter(|(key, _)| key == "expire")
        .filter_map(|(_, value)| value.parse::<u64>().ok())
        .collect::<Vec<_>>();
    if n != 1 || expires.len() != 1 || expires[0] <= now_unix().saturating_add(300) {
        return Err(Error::new(
            ErrorCode::Unavailable,
            "cipher n or fresh expiry unavailable",
        ));
    }
    let title = response["videoDetails"]["title"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .unwrap_or(&media_id)
        .to_owned();
    let duration = response["videoDetails"]["lengthSeconds"]
        .as_str()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0 && *value <= 600.0);
    let provider: Arc<dyn MetadataProvider> = Arc::new(CapturedCipher {
        youtube: Youtube::new()?,
        media_id,
        raw_format: (*raw[0]).clone(),
        player: Arc::new(
            String::from_utf8(player_bytes)
                .map_err(|_| Error::new(ErrorCode::Protocol, "player source is not UTF-8"))?,
        ),
        title,
        duration,
        expires: expires[0],
    });
    support::run_with_provider(provider, false).await
}
