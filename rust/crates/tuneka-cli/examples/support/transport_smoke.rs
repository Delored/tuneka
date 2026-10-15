use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tuneka_core::*;
use tuneka_youtube::{TransportPreference, Youtube};

#[allow(dead_code)] // Some explicit QA examples supply their own provider wrapper.
pub async fn run(transport: TransportPreference) -> Result<()> {
    run_with_provider(Arc::new(Youtube::with_transport(transport)?), true).await
}

pub async fn run_with_provider(
    metadata: Arc<dyn MetadataProvider>,
    require_segmented: bool,
) -> Result<()> {
    let root = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .ok_or_else(|| Error::new(ErrorCode::InvalidInput, "isolated output root required"))?;
    let video = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "FZ8BxMU3BYc".into());
    if root.exists() {
        return Err(Error::new(ErrorCode::Conflict, "smoke root must be new"));
    }
    let format: OutputFormat = std::env::args()
        .nth(3)
        .unwrap_or_else(|| "mp4".into())
        .parse()
        .map_err(|_| Error::new(ErrorCode::InvalidInput, "invalid output format"))?;
    let exercise_pause = std::env::args().nth(4).as_deref() == Some("pause");
    let url = format!("https://www.youtube.com/watch?v={video}");
    let info = metadata.inspect(&url).await?;
    let request = DownloadRequest {
        urls: vec![url],
        output_dir: root.join("outputs"),
        format,
        max_height: Some(if format == OutputFormat::Mkv {
            144
        } else {
            360
        }),
        chapters: vec![],
        playlist_items: vec![],
    };
    let (video_format, audio_format) = select_formats(&info, &request)?;
    for format in std::iter::once(&video_format).chain(audio_format.iter()) {
        let resolved = metadata.resolve_stream(&video, &format.id).await?;
        if let Some(plan) = &resolved.segmented {
            println!(
                "{}",
                serde_json::json!({"phase":"resolved","format":format,"segments_including_init":plan.segments.len(),"identity":plan.identity})
            );
        } else if let Some(sabr) = &resolved.sabr {
            println!(
                "{}",
                serde_json::json!({"phase":"resolved","format":format,"transport":"sabr","video_itag":sabr.video.itag,"audio_itag":sabr.audio.itag})
            );
        } else if require_segmented {
            return Err(Error::new(ErrorCode::Protocol, "not a segmented transport"));
        } else {
            println!(
                "{}",
                serde_json::json!({"phase":"resolved","format":format,"transport":"direct"})
            );
        }
    }
    let engine = Engine::new(
        metadata,
        Arc::new(tuneka_download::HttpDownloader::new()?),
        Arc::new(tuneka_cli::FfmpegProcessor::default()),
        root.join("state"),
    )?;
    let id = engine.create(request)?;
    let start = Instant::now();
    let mut previous = None;
    let mut pause_requested = false;
    let mut resumed = false;
    loop {
        let job = engine.get(&id)?;
        if previous != Some(job.state) {
            println!("{}", serde_json::json!({"phase":"state","state":job.state}));
            previous = Some(job.state);
        }
        if exercise_pause
            && !pause_requested
            && job.state == JobState::Downloading
            && job.progress.bytes > 65536
        {
            engine.pause(&id)?;
            pause_requested = true;
        }
        if pause_requested && !resumed && job.state == JobState::Paused {
            println!(
                "{}",
                serde_json::json!({"phase":"pause_acknowledged","bytes":job.progress.bytes})
            );
            engine.resume(&id, true)?;
            resumed = true;
        }
        if job.state.terminal() {
            println!("{}", serde_json::to_string(&job)?);
            engine.shutdown().await;
            if exercise_pause && !resumed {
                return Err(Error::new(
                    ErrorCode::Protocol,
                    "pause/resume was not exercised",
                ));
            }
            if job.state != JobState::Completed {
                return Err(Error::new(
                    ErrorCode::Unavailable,
                    "segmented job did not complete",
                ));
            }
            break;
        }
        if start.elapsed() > Duration::from_secs(180) || job.progress.bytes > 100 * 1024 * 1024 {
            engine.cancel(&id)?;
            engine.shutdown().await;
            return Err(Error::new(
                ErrorCode::Cancelled,
                "live validation limit reached",
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}
