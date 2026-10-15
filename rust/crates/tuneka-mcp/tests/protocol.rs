use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
#[tokio::test]
async fn actual_stdio_lifecycle_schemas_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_tuneka-mcp")).configure(|c| {
            c.env("TUNEKA_RUST_DATA_DIR", dir.path().join("state"))
                .env("TUNEKA_DOWNLOADS", dir.path().join("outputs"));
        }),
    )
    .unwrap();
    let client = ().serve(transport).await.unwrap();
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 9);
    for name in [
        "inspect_media",
        "list_formats",
        "list_playlist_items",
        "create_download",
        "get_job",
        "list_jobs",
        "pause_job",
        "resume_job",
        "cancel_job",
    ] {
        assert!(tools.iter().any(|t| t.name == name));
    }
    let schema = tools.iter().find(|t| t.name == "create_download").unwrap();
    assert!(
        !serde_json::to_string(&schema.input_schema)
            .unwrap()
            .contains("output_dir")
    );
    let result = client
        .call_tool(CallToolRequestParams::new("list_jobs"))
        .await
        .unwrap();
    assert_ne!(result.is_error, Some(true));
    assert_eq!(result.structured_content, Some(serde_json::json!([])));
    let result = client
        .call_tool(
            CallToolRequestParams::new("create_download").with_arguments(
                serde_json::json!({"urls":[],"format":"mp3"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    let result = client
        .call_tool(
            CallToolRequestParams::new("inspect_media").with_arguments(
                serde_json::json!({"url":"http://localhost/secret"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    let result = client
        .call_tool(
            CallToolRequestParams::new("get_job").with_arguments(
                serde_json::json!({"job_id":"unknown"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    client.cancel().await.unwrap();
}
