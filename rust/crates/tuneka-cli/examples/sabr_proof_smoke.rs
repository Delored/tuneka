//! Explicit QA only: OUTPUT_ROOT VIDEO_ID [mp4|mkv] [pause].
//! TUNEKA_SABR_PROOF_FILE points to a private JSON containing a server-issued,
//! video-bound WEB proof. No token acquisition dependency is part of the product.
#[path = "support/transport_smoke.rs"]
mod support;

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use std::{path::PathBuf, sync::Arc, time::SystemTime};
use tuneka_core::*;
use tuneka_youtube::{TransportPreference, Youtube};

struct ProofProvider {
    inner: Youtube,
    video_id: String,
    token: Vec<u8>,
    expires_ms: u64,
}

fn now_ms() -> Result<u64> {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .ok_or_else(|| Error::new(ErrorCode::InvalidInput, "system clock unavailable"))
}

impl ProofProvider {
    fn read(path: PathBuf, video_id: String) -> Result<Self> {
        let invalid = || Error::new(ErrorCode::InvalidInput, "invalid or expired WEB proof file");
        let file = std::fs::File::open(path)?;
        if file.metadata()?.len() > 16_384 {
            return Err(invalid());
        }
        // Bounded even if the source file changes after metadata inspection.
        use std::io::Read;
        let mut bytes = Vec::new();
        file.take(16_385).read_to_end(&mut bytes)?;
        if bytes.len() > 16_384 {
            return Err(invalid());
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if value["video_id"].as_str() != Some(video_id.as_str())
            || value["client"].as_str() != Some("WEB")
        {
            return Err(invalid());
        }
        let created = value["created_at"].as_u64().ok_or_else(invalid)?;
        let ttl = value["ttl_seconds"].as_u64().ok_or_else(invalid)?;
        // GenerateIT reports the minter's integrity-token TTL, not a guarantee
        // of this individual proof's validity. Keep this explicit QA run fresh.
        let expires_ms = ttl
            .min(900)
            .checked_mul(1000)
            .and_then(|ttl| created.checked_add(ttl))
            .ok_or_else(invalid)?;
        let now = now_ms()?;
        if created > now || expires_ms <= now || ttl > 86_400 {
            return Err(invalid());
        }
        let token = URL_SAFE_NO_PAD
            .decode(value["po_token"].as_str().ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
        if token.is_empty() || token.len() > 4096 {
            return Err(invalid());
        }
        Ok(Self {
            inner: Youtube::with_transport(TransportPreference::Sabr)?,
            video_id,
            token,
            expires_ms,
        })
    }
}

#[async_trait]
impl MetadataProvider for ProofProvider {
    async fn inspect(&self, url: &str) -> Result<MediaInfo> {
        self.inner.inspect(url).await
    }
    async fn playlist_page(&self, url: &str, continuation: Option<&str>) -> Result<PlaylistPage> {
        self.inner.playlist_page(url, continuation).await
    }
    async fn fetch_thumbnail(&self, url: &str, target: PathBuf) -> Result<PathBuf> {
        self.inner.fetch_thumbnail(url, target).await
    }
    async fn resolve_stream(&self, media_id: &str, format_id: &str) -> Result<ResolvedStream> {
        if media_id != self.video_id || now_ms()? >= self.expires_ms {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "proof binding expired or mismatched",
            ));
        }
        let mut stream = self.inner.resolve_stream(media_id, format_id).await?;
        stream
            .sabr
            .as_mut()
            .ok_or_else(|| Error::new(ErrorCode::Protocol, "expected WEB SABR stream"))?
            .po_token = Some(self.token.clone());
        Ok(stream)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let path = std::env::var_os("TUNEKA_SABR_PROOF_FILE")
        .ok_or_else(|| Error::new(ErrorCode::InvalidInput, "private proof file required"))?;
    let video = std::env::args()
        .nth(2)
        .ok_or_else(|| Error::new(ErrorCode::InvalidInput, "explicit video ID required"))?;
    let provider = ProofProvider::read(path.into(), video)?;
    support::run_with_provider(Arc::new(provider), true).await
}
