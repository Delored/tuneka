use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};
use tuneka_core::{DownloadRequest, Engine, Error, ErrorCode};
#[derive(Clone)]
pub struct TunekaServer {
    engine: Arc<Engine>,
    output_root: PathBuf,
    tool_router: ToolRouter<Self>,
}
impl TunekaServer {
    pub fn new(engine: Arc<Engine>, output_root: PathBuf) -> tuneka_core::Result<Self> {
        std::fs::create_dir_all(&output_root)?;
        Ok(Self {
            engine,
            output_root: std::fs::canonicalize(output_root)?,
            tool_router: Self::tool_router(),
        })
    }
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct UrlParams {
    pub url: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct PlaylistParams {
    pub url: String,
    pub continuation: Option<String>,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct CreateParams {
    pub urls: Vec<String>,
    #[schemars(description = "mp3, m4a, flac, wav, mp4, mkv, or webm")]
    pub format: String,
    pub max_height: Option<u32>,
    #[serde(default)]
    pub chapters: Vec<usize>,
    #[serde(default)]
    pub playlist_items: Vec<usize>,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct JobParams {
    pub job_id: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ResumeParams {
    pub job_id: String,
    #[serde(default)]
    pub retry_failed: bool,
}
fn response<T: serde::Serialize>(r: tuneka_core::Result<T>) -> CallToolResult {
    match r {
        Ok(value) => CallToolResult::structured(serde_json::to_value(value).unwrap_or_default()),
        Err(e) => CallToolResult::structured_error(serde_json::json!({"error":e})),
    }
}
#[tool_router]
impl TunekaServer {
    #[tool(description = "Inspect a public YouTube video or Short. Does not download.")]
    async fn inspect_media(&self, Parameters(p): Parameters<UrlParams>) -> CallToolResult {
        response(self.engine.inspect(&p.url).await)
    }
    #[tool(description = "List audio/video format metadata; signed stream URLs are never exposed.")]
    async fn list_formats(&self, Parameters(p): Parameters<UrlParams>) -> CallToolResult {
        response(self.engine.inspect(&p.url).await.map(|m| m.formats))
    }
    #[tool(
        description = "Read one playlist page. Follow continuation until complete=true. Page failures are errors."
    )]
    async fn list_playlist_items(
        &self,
        Parameters(p): Parameters<PlaylistParams>,
    ) -> CallToolResult {
        response(
            self.engine
                .playlist_page(&p.url, p.continuation.as_deref())
                .await,
        )
    }
    #[tool(
        description = "Create an asynchronous download in the configured output root. Returns job_id; poll get_job. Chapter/item indices are one-based."
    )]
    async fn create_download(&self, Parameters(p): Parameters<CreateParams>) -> CallToolResult {
        response(
            p.format
                .parse()
                .map_err(|e| Error::new(ErrorCode::InvalidInput, e))
                .and_then(|format| {
                    self.engine.create(DownloadRequest {
                        urls: p.urls,
                        output_dir: self.output_root.clone(),
                        format,
                        max_height: p.max_height,
                        chapters: p.chapters,
                        playlist_items: p.playlist_items,
                    })
                })
                .map(|id| serde_json::json!({"job_id":id})),
        )
    }
    #[tool(
        description = "Read authoritative aggregate state and per-item outcomes, artifacts, warnings and errors."
    )]
    async fn get_job(&self, Parameters(p): Parameters<JobParams>) -> CallToolResult {
        response(self.engine.get(&p.job_id))
    }
    #[tool(description = "List jobs owned by this server process.")]
    async fn list_jobs(&self) -> CallToolResult {
        response(Ok(self.engine.list()))
    }
    #[tool(
        description = "Request cooperative pause. Poll until paused; no new work continues after pause acknowledgement."
    )]
    async fn pause_job(&self, Parameters(p): Parameters<JobParams>) -> CallToolResult {
        response(
            self.engine
                .pause(&p.job_id)
                .and_then(|_| self.engine.get(&p.job_id)),
        )
    }
    #[tool(
        description = "Resume a paused/persisted job; retry_failed explicitly opts into retrying failed/partial/cancelled jobs. Completed items remain intact."
    )]
    async fn resume_job(&self, Parameters(p): Parameters<ResumeParams>) -> CallToolResult {
        response(
            self.engine
                .restore_in_root(&p.job_id, p.retry_failed, &self.output_root)
                .and_then(|_| self.engine.get(&p.job_id)),
        )
    }
    #[tool(
        description = "Cancel a job cooperatively, retaining already published outputs. Poll for acknowledgement."
    )]
    async fn cancel_job(&self, Parameters(p): Parameters<JobParams>) -> CallToolResult {
        response(
            self.engine
                .cancel(&p.job_id)
                .and_then(|_| self.engine.get(&p.job_id)),
        )
    }
}
#[tool_handler(router = self.tool_router)]
impl ServerHandler for TunekaServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions("Tuneka downloads public YouTube media. Jobs run only while this server is connected. Inspect per-item results: partially_completed is not success. Outputs are restricted to the configured server root.")
    }
}
