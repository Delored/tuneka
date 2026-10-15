use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tuneka_core::*;
#[derive(Parser)]
#[command(
    name = "tuneka",
    about = "YouTube media downloader using Rust and linked FFmpeg libraries",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Inspect {
        url: String,
        #[arg(long)]
        json: bool,
    },
    Playlist {
        url: String,
        #[arg(long)]
        continuation: Option<String>,
    },
    Download {
        #[arg(required = true)]
        urls: Vec<String>,
        #[arg(long, conflicts_with = "video")]
        audio: Option<String>,
        #[arg(long)]
        video: bool,
        #[arg(long, default_value = "mp4", requires = "video")]
        container: String,
        #[arg(long, default_value = "best")]
        quality: String,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, value_delimiter = ',')]
        chapters: Vec<usize>,
        #[arg(long, value_delimiter = ',')]
        items: Vec<usize>,
        #[arg(long)]
        json: bool,
    },
    Resume {
        job_id: String,
        #[arg(long)]
        retry_failed: bool,
    },
    Cancel {
        job_id: String,
    },
    ImportSettings {
        path: PathBuf,
    },
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run(Cli::parse()).await {
        Ok(code) => std::process::ExitCode::from(code),
        Err(e) => {
            eprintln!(
                "{}",
                serde_json::to_string(&e).unwrap_or_else(|_| e.to_string())
            );
            std::process::ExitCode::from(1)
        }
    }
}
async fn run(cli: Cli) -> Result<u8> {
    if let Command::ImportSettings { path } = cli.command {
        let settings = settings::import_python(&settings::data_dir().join("settings.json"), &path)?;
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(0);
    }
    let engine = tuneka_cli::engine()?;
    match cli.command {
        Command::Inspect { url, json } => {
            let info = engine.inspect(&url).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&info)?);
            } else {
                println!(
                    "{}\n{} formats; {} chapters",
                    info.title,
                    info.formats.len(),
                    info.chapters.len()
                );
            }
            Ok(0)
        }
        Command::Playlist { url, continuation } => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &engine.playlist_page(&url, continuation.as_deref()).await?
                )?
            );
            Ok(0)
        }
        Command::Download {
            urls,
            audio,
            video,
            container,
            quality,
            output,
            chapters,
            items,
            json: _,
        } => {
            let format = (if video {
                container.as_str()
            } else {
                audio.as_deref().unwrap_or("mp3")
            })
            .parse()
            .map_err(|e| Error::new(ErrorCode::InvalidInput, e))?;
            if video && OutputFormat::is_audio(format) {
                return Err(Error::new(
                    ErrorCode::InvalidInput,
                    "video requires mp4/mkv/webm",
                ));
            }
            if !video && !OutputFormat::is_audio(format) {
                return Err(Error::new(
                    ErrorCode::InvalidInput,
                    "audio requires mp3/m4a/flac/wav",
                ));
            }
            let max_height = if quality == "best" {
                None
            } else {
                Some(quality.trim_end_matches('p').parse::<u32>().map_err(|_| {
                    Error::new(
                        ErrorCode::InvalidInput,
                        "quality must be best or a height such as 1080p",
                    )
                })?)
            };
            let defaults = settings::update(&settings::data_dir().join("settings.json"), |_| {})?;
            let id = engine.create(DownloadRequest {
                urls,
                output_dir: output.unwrap_or(defaults.output_dir),
                format,
                max_height,
                chapters,
                playlist_items: items,
            })?;
            wait(engine, id).await
        }
        Command::Cancel { job_id } => {
            let snapshot = engine.cancel_saved(&job_id)?;
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
            Ok(130)
        }
        Command::Resume {
            job_id,
            retry_failed,
        } => {
            engine.restore(&job_id, retry_failed)?;
            wait(engine, job_id).await
        }
        Command::ImportSettings { .. } => unreachable!(),
    }
}
async fn wait(engine: std::sync::Arc<Engine>, id: String) -> Result<u8> {
    eprintln!("job_id: {id}");
    let snapshot = tokio::select! {r=engine.wait(&id)=>r?,_=tokio::signal::ctrl_c()=>{engine.pause(&id)?;engine.wait(&id).await?}};
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(tuneka_cli::exit_code(&snapshot))
}
