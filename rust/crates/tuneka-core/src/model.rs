use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidInput,
    Unsupported,
    Unavailable,
    Network,
    Protocol,
    Conflict,
    Io,
    Cancelled,
    Paused,
    Media,
    Internal,
}
impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::new(ErrorCode::Io, e.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::new(ErrorCode::Protocol, e.to_string())
    }
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub title: String,
    pub start: f64,
    pub end: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Format {
    pub id: String,
    pub mime_type: String,
    pub codecs: Vec<String>,
    pub has_audio: bool,
    pub has_video: bool,
    pub bitrate: u64,
    pub height: Option<u32>,
    pub content_length: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    pub id: String,
    pub title: String,
    pub duration: Option<f64>,
    pub chapters: Vec<Chapter>,
    pub has_chapters: bool,
    pub thumbnails: Vec<String>,
    pub formats: Vec<Format>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistItem {
    pub id: String,
    pub title: String,
    pub available: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistPage {
    pub title: String,
    pub items: Vec<PlaylistItem>,
    pub continuation: Option<String>,
    pub complete: bool,
}
#[derive(Clone)]
pub struct ResolvedStream {
    pub media_id: String,
    pub format: Format,
    pub url: String,
    /// Finite VOD segments, resolved afresh with each URL refresh.
    pub segmented: Option<SegmentedStream>,
    /// Ephemeral SABR session inputs; no URLs/configuration are persisted.
    pub sabr: Option<SabrStream>,
}
#[derive(Clone)]
pub struct SabrStream {
    pub client_version: String,
    pub user_agent: String,
    pub video_playback_config: Vec<u8>,
    /// Optional server-issued, video-bound proof; kept in memory only.
    /// The provider must refresh it on expiry and bind it to this WEB media session.
    pub po_token: Option<Vec<u8>>,
    pub video: SabrFormat,
    pub audio: SabrFormat,
    pub selected_itag: u32,
    pub duration_ms: u64,
}
#[derive(Clone)]
pub struct SabrFormat {
    pub itag: u32,
    pub last_modified: u64,
    pub xtags: Option<String>,
    pub content_length: u64,
}
// Signed stream URLs intentionally have no Debug/Serialize implementation.
#[derive(Clone)]
pub struct SegmentedStream {
    /// SHA-256 of stable representation/segment identities, never signed URLs.
    pub identity: String,
    pub segments: Vec<MediaSegment>,
}
#[derive(Clone)]
pub struct MediaSegment {
    pub url: String,
    /// Inclusive HTTP byte range (initialization segments included in order).
    pub byte_range: Option<(u64, u64)>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Mp3,
    M4a,
    Flac,
    Wav,
    Mp4,
    Mkv,
    Webm,
}
impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::M4a => "m4a",
            Self::Flac => "flac",
            Self::Wav => "wav",
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
        }
    }
    pub fn is_audio(self) -> bool {
        matches!(self, Self::Mp3 | Self::M4a | Self::Flac | Self::Wav)
    }
}
impl std::str::FromStr for OutputFormat {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s {
            "mp3" => Ok(Self::Mp3),
            "m4a" => Ok(Self::M4a),
            "flac" => Ok(Self::Flac),
            "wav" => Ok(Self::Wav),
            "mp4" => Ok(Self::Mp4),
            "mkv" => Ok(Self::Mkv),
            "webm" => Ok(Self::Webm),
            _ => Err("unsupported output format".into()),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRequest {
    pub urls: Vec<String>,
    pub output_dir: PathBuf,
    pub format: OutputFormat,
    pub max_height: Option<u32>,
    #[serde(default)]
    pub chapters: Vec<usize>,
    #[serde(default)]
    pub playlist_items: Vec<usize>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Resolving,
    Downloading,
    Processing,
    Paused,
    Completed,
    PartiallyCompleted,
    Failed,
    Cancelled,
}
impl JobState {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::PartiallyCompleted | Self::Failed | Self::Cancelled
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub path: PathBuf,
    pub size: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemOutcome {
    pub index: usize,
    pub url: String,
    pub title: String,
    pub state: JobState,
    pub artifacts: Vec<Artifact>,
    pub error: Option<Error>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Progress {
    pub bytes: u64,
    pub total: Option<u64>,
    pub bytes_per_second: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSnapshot {
    pub id: String,
    pub revision: u64,
    pub state: JobState,
    pub request: DownloadRequest,
    pub enumeration_complete: bool,
    pub items: Vec<ItemOutcome>,
    pub progress: Progress,
    pub warnings: Vec<String>,
    pub error: Option<Error>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YoutubeUrl {
    Video(String),
    Playlist(String),
}
pub fn parse_url(value: &str) -> Result<YoutubeUrl> {
    let u =
        url::Url::parse(value).map_err(|_| Error::new(ErrorCode::InvalidInput, "invalid URL"))?;
    let host = u.host_str().unwrap_or("").trim_end_matches('.');
    if u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some_and(|p| p != 443)
        || !(host == "youtu.be" || host == "youtube.com" || host.ends_with(".youtube.com"))
    {
        return Err(Error::new(
            ErrorCode::InvalidInput,
            "only HTTPS YouTube URLs are supported",
        ));
    }
    let parts: Vec<_> = u.path().trim_matches('/').split('/').collect();
    if parts.first() == Some(&"live") {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "live streams are unsupported",
        ));
    }
    let id = if host == "youtu.be" {
        parts.first().map(|s| s.to_string())
    } else if parts == ["watch"] {
        u.query_pairs()
            .find(|(k, _)| k == "v")
            .map(|(_, v)| v.into_owned())
    } else if parts.len() == 2 && matches!(parts[0], "shorts" | "embed" | "v") {
        Some(parts[1].into())
    } else {
        None
    };
    if let Some(id) = id
        && valid_id(&id, 11, 11)
    {
        return Ok(YoutubeUrl::Video(id));
    }
    if let Some((_, id)) = u.query_pairs().find(|(k, _)| k == "list")
        && valid_id(&id, 10, 100)
    {
        return Ok(YoutubeUrl::Playlist(id.into_owned()));
    }
    Err(Error::new(
        ErrorCode::InvalidInput,
        "URL has no valid video or playlist ID",
    ))
}
fn valid_id(id: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&id.len())
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
pub fn normalize_chapters(
    title: &str,
    duration: Option<f64>,
    mut chapters: Vec<Chapter>,
) -> Result<Vec<Chapter>> {
    let duration = duration.filter(|v| v.is_finite() && *v > 0.0);
    if chapters.is_empty() {
        return Ok(vec![Chapter {
            title: title.into(),
            start: 0.0,
            end: duration,
        }]);
    }
    for i in 0..chapters.len() {
        let start = chapters[i].start;
        if !start.is_finite()
            || start < 0.0
            || (i > 0 && start <= chapters[i - 1].start)
            || duration.is_some_and(|d| start >= d)
        {
            return Err(Error::new(
                ErrorCode::Protocol,
                "invalid chapter boundaries",
            ));
        }
        let next = chapters.get(i + 1).map(|c| c.start).or(duration);
        let end = chapters[i]
            .end
            .filter(|v| v.is_finite() && *v > start)
            .or(next);
        chapters[i].end = match (end, next) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, _) => a,
        };
    }
    Ok(chapters)
}
