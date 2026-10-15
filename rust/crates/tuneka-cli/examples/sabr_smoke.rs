//! Explicit bounded live SABR check: OUTPUT_ROOT [video-id] [mp4|mkv] [pause].
#[path = "support/transport_smoke.rs"]
mod support;
#[tokio::main]
async fn main() -> tuneka_core::Result<()> {
    support::run(tuneka_youtube::TransportPreference::Sabr).await
}
