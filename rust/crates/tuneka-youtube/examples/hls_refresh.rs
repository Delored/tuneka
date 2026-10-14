//! Explicit live check: report only changed URL field names, never values/URLs.
use std::collections::{BTreeMap, BTreeSet};
use tuneka_core::*;
use tuneka_youtube::Youtube;
fn fields(value: &str) -> BTreeMap<String, String> {
    let url = url::Url::parse(value).unwrap();
    let mut fields = BTreeMap::new();
    let path = url.path().strip_prefix("/videoplayback/").unwrap_or("");
    let parts: Vec<_> = path.split('/').collect();
    for pair in parts.chunks_exact(2) {
        fields.insert(pair[0].into(), pair[1].into());
    }
    for (key, value) in url.query_pairs() {
        fields.insert(key.into_owned(), value.into_owned());
    }
    fields
}
#[tokio::main]
async fn main() -> Result<()> {
    let youtube = Youtube::new()?;
    let format_id = std::env::args().nth(1).unwrap_or_else(|| "hls-603".into());
    let a = youtube
        .resolve_stream("FZ8BxMU3BYc", &format_id)
        .await?
        .segmented
        .unwrap();
    let b = youtube
        .resolve_stream("FZ8BxMU3BYc", &format_id)
        .await?
        .segmented
        .unwrap();
    let mut changed = BTreeSet::new();
    for (a, b) in a.segments.iter().zip(&b.segments) {
        let (a, b) = (fields(&a.url), fields(&b.url));
        for key in a.keys().chain(b.keys()) {
            if a.get(key) != b.get(key) {
                changed.insert(key.clone());
            }
        }
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap();
    let mut anchors = Vec::new();
    for index in [0, 1, a.segments.len() - 1] {
        let mut hashes = Vec::new();
        for segment in [&a.segments[index], &b.segments[index]] {
            let mut request = client.get(&segment.url);
            if let Some((start, end)) = segment.byte_range {
                request = request.header("Range", format!("bytes={start}-{end}"));
            }
            let mut response = request
                .send()
                .await
                .map_err(|_| Error::new(ErrorCode::Network, "anchor request failed"))?;
            if !response.status().is_success() {
                return Err(Error::new(ErrorCode::Network, "anchor HTTP failed"));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| Error::new(ErrorCode::Network, "anchor read failed"))?
            {
                if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                    return Err(Error::new(ErrorCode::Protocol, "anchor byte limit"));
                }
                bytes.extend_from_slice(&chunk);
            }
            use sha2::{Digest, Sha256};
            hashes.push((bytes.len(), format!("{:x}", Sha256::digest(&bytes))));
        }
        anchors.push(serde_json::json!({"segment_index":index,"same_bytes":hashes[0]==hashes[1],"length":hashes[0].0}));
    }
    println!(
        "{}",
        serde_json::json!({"anchor_comparison":anchors,"same_identity":a.identity==b.identity,"segment_counts":[a.segments.len(),b.segments.len()],"changed_field_names":changed})
    );
    Ok(())
}
