//! Explicit bounded live validation; never run by offline tests.
//! Usage: cargo run -p tuneka-cli --example dash_smoke -- OUTPUT_ROOT [video-id] [mp4|mkv] [pause]
#[path = "support/transport_smoke.rs"]
mod support;
#[tokio::main]
async fn main() -> tuneka_core::Result<()> {
    support::run(tuneka_youtube::TransportPreference::Dash).await
}
