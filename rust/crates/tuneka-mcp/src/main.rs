use rmcp::ServiceExt;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = tuneka_cli::engine()?;
    let root = std::env::var_os("TUNEKA_DOWNLOADS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("downloads"));
    let server = tuneka_mcp::TunekaServer::new(engine.clone(), root)?;
    let running = server.serve(rmcp::transport::stdio()).await?;
    let result = running.waiting().await;
    engine.shutdown().await;
    result?;
    Ok(())
}
