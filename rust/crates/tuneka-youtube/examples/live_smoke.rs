//! Run explicitly: cargo run -p tuneka-youtube --example live_smoke -- URL [--probe]
use tuneka_core::{MetadataProvider, Result, YoutubeUrl, parse_url};
use tuneka_youtube::Youtube;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let url = args
        .first()
        .map(String::as_str)
        .unwrap_or("https://www.youtube.com/watch?v=FZ8BxMU3BYc");
    let youtube = Youtube::new()?;
    match parse_url(url)? {
        YoutubeUrl::Playlist(id) => {
            let page = youtube.playlist_page(url, None).await?;
            println!(
                "playlist={id} page=1 items={} complete={}",
                page.items.len(),
                page.complete
            );
            if let Some(token) = page.continuation {
                let next = youtube.playlist_page(url, Some(&token)).await?;
                println!(
                    "playlist={id} page=2 items={} complete={}",
                    next.items.len(),
                    next.complete
                );
            }
        }
        YoutubeUrl::Video(id) => {
            let info = youtube.inspect(url).await?;
            println!(
                "video={id} duration={:?} formats={} chapters={} has_chapters={}",
                info.duration,
                info.formats.len(),
                info.chapters.len(),
                info.has_chapters
            );
            if args.iter().any(|arg| arg == "--probe") {
                for kind in ["audio", "video"] {
                    let format = info
                        .formats
                        .iter()
                        .filter(|f| {
                            if kind == "audio" {
                                f.has_audio && !f.has_video
                            } else {
                                f.has_video && !f.has_audio
                            }
                        })
                        .min_by_key(|f| f.bitrate)
                        .expect("live video must offer requested stream kind");
                    let stream = youtube.resolve_stream(&id, &format.id).await?;
                    let bytes = youtube.probe_stream(&stream, 16384).await?;
                    println!("video={id} kind={kind} format={} bytes={bytes}", format.id);
                }
            }
        }
    }
    Ok(())
}
