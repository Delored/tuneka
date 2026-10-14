use crate::*;
use async_trait::async_trait;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
#[derive(Clone, Default)]
pub struct Control(Arc<AtomicU8>);
impl Control {
    pub fn pause(&self) {
        let _ = self
            .0
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst);
    }
    pub fn cancel(&self) {
        self.0.store(2, Ordering::SeqCst);
    }
    pub fn check(&self) -> Result<()> {
        match self.0.load(Ordering::SeqCst) {
            1 => Err(Error::new(ErrorCode::Paused, "pause acknowledged")),
            2 => Err(Error::new(ErrorCode::Cancelled, "cancel acknowledged")),
            _ => Ok(()),
        }
    }
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst) == 2
    }
}
pub type ProgressSink = Arc<dyn Fn(Progress) + Send + Sync>;
#[async_trait]
pub trait MetadataProvider: Send + Sync {
    async fn fetch_thumbnail(&self, _url: &str, _target: PathBuf) -> Result<PathBuf> {
        Err(Error::new(
            ErrorCode::Unsupported,
            "thumbnail download unavailable",
        ))
    }
    async fn inspect(&self, url: &str) -> Result<MediaInfo>;
    async fn playlist_page(&self, url: &str, continuation: Option<&str>) -> Result<PlaylistPage>;
    async fn resolve_stream(&self, media_id: &str, format_id: &str) -> Result<ResolvedStream>;
}
#[async_trait]
pub trait Downloader: Send + Sync {
    async fn download(
        &self,
        stream: ResolvedStream,
        target: PathBuf,
        control: Control,
        progress: ProgressSink,
    ) -> Result<PathBuf>;
}
#[derive(Clone)]
pub struct ProcessRequest {
    pub publication_receipt: PathBuf,
    pub input: PathBuf,
    pub additional_audio: Option<PathBuf>,
    pub output: PathBuf,
    pub format: OutputFormat,
    pub chapter: Option<Chapter>,
    pub title: String,
    pub cover: Option<PathBuf>,
}
#[async_trait]
pub trait MediaProcessor: Send + Sync {
    async fn process(
        &self,
        request: ProcessRequest,
        control: Control,
    ) -> Result<(Artifact, Vec<String>)>;
}
