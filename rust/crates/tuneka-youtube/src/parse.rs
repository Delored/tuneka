use serde::Deserialize;
use serde_json::Value;
use tuneka_core::*;

pub fn assigned_json(html: &str, name: &str) -> Result<Value> {
    let window_double = format!("window[\"{name}\"]");
    let window_single = format!("window['{name}']");
    let window_dot = format!("window.{name}");
    let scripts =
        regex::Regex::new(r"(?is)<script\b[^>]*>(.*?)</script\s*>").expect("static regex");
    for capture in scripts.captures_iter(html) {
        let script = capture.get(1).expect("script capture").as_str();
        let mut i = 0;
        let bytes = script.as_bytes();
        while i < bytes.len() {
            // Only inspect executable assignment positions, never HTML attributes,
            // JavaScript strings or comments containing user-provided titles.
            if matches!(bytes[i], b'\'' | b'"' | b'`') {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == quote {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            if bytes[i..].starts_with(b"//") {
                i += script[i..].find('\n').unwrap_or(script.len() - i);
                continue;
            }
            if bytes[i..].starts_with(b"/*") {
                i += script[i + 2..]
                    .find("*/")
                    .map(|n| n + 4)
                    .unwrap_or(script.len() - i);
                continue;
            }
            let boundary =
                i == 0 || !matches!(bytes[i-1],b'a'..=b'z'|b'A'..=b'Z'|b'0'..=b'9'|b'_'|b'$');
            if boundary {
                let remaining = &script[i..];
                let marker = [
                    name,
                    window_double.as_str(),
                    window_single.as_str(),
                    window_dot.as_str(),
                ]
                .into_iter()
                .find(|marker| {
                    remaining.starts_with(marker)
                        && remaining
                            .as_bytes()
                            .get(marker.len())
                            .is_none_or(|c| !c.is_ascii_alphanumeric() && !matches!(c, b'_' | b'$'))
                });
                if let Some(marker) = marker {
                    let rest = remaining[marker.len()..].trim_start();
                    if let Some(input) = rest.strip_prefix('=')
                        && !input.starts_with('=')
                    {
                        let mut decoder = serde_json::Deserializer::from_str(input.trim_start());
                        if let Ok(value) = Value::deserialize(&mut decoder)
                            && value.is_object()
                        {
                            return Ok(value);
                        }
                    }
                }
            }
            i += 1;
            // Byte scanning may cross a UTF-8 title character; assignments are ASCII.
            while i < bytes.len() && !script.is_char_boundary(i) {
                i += 1;
            }
        }
    }
    Err(Error::new(
        ErrorCode::Protocol,
        format!("missing {name} JSON"),
    ))
}

pub fn json_string_field(html: &str, name: &str) -> Option<String> {
    let marker = format!("\"{name}\":");
    let start = html.find(&marker)? + marker.len();
    let mut decoder = serde_json::Deserializer::from_str(html[start..].trim_start());
    String::deserialize(&mut decoder).ok()
}
pub fn json_object_field(html: &str, name: &str) -> Option<Value> {
    let marker = format!("\"{name}\":");
    let start = html.find(&marker)? + marker.len();
    let mut decoder = serde_json::Deserializer::from_str(html[start..].trim_start());
    Value::deserialize(&mut decoder)
        .ok()
        .filter(Value::is_object)
}

pub fn check_playability(response: &Value) -> Result<()> {
    let status = response
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str);
    if status != Some("OK") {
        let code = if matches!(status, Some("LOGIN_REQUIRED" | "AGE_CHECK_REQUIRED")) {
            ErrorCode::Unsupported
        } else {
            ErrorCode::Unavailable
        };
        return Err(Error::new(
            code,
            "video is unavailable, restricted or requires authentication",
        ));
    }
    let details = &response["videoDetails"];
    if details["isLiveContent"].as_bool() == Some(true)
        || details["isUpcoming"].as_bool() == Some(true)
        || response.pointer("/streamingData/hlsManifestUrl").is_some()
            && details["lengthSeconds"].as_str() == Some("0")
    {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "live and upcoming streams are unsupported",
        ));
    }
    if response.pointer("/streamingData/licenseInfos").is_some() {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "DRM-protected media is unsupported",
        ));
    }
    Ok(())
}

pub fn raw_formats(response: &Value) -> Vec<&Value> {
    ["formats", "adaptiveFormats"]
        .into_iter()
        .flat_map(|key| {
            response["streamingData"][key]
                .as_array()
                .into_iter()
                .flatten()
        })
        .collect()
}
pub fn find_format<'a>(response: &'a Value, id: &str) -> Option<&'a Value> {
    raw_formats(response).into_iter().find(|format| {
        format["itag"]
            .as_u64()
            .is_some_and(|itag| itag.to_string() == id)
    })
}
pub fn has_transport(format: &Value) -> bool {
    ["url", "signatureCipher", "cipher"]
        .into_iter()
        .any(|key| format[key].as_str().is_some_and(|s| !s.is_empty()))
}
pub fn format(value: &Value) -> Result<Format> {
    let id = value["itag"]
        .as_u64()
        .ok_or_else(|| Error::new(ErrorCode::Protocol, "format lacks itag"))?
        .to_string();
    let mime_type = value["mimeType"]
        .as_str()
        .ok_or_else(|| Error::new(ErrorCode::Protocol, "format lacks MIME type"))?
        .to_owned();
    let codecs: Vec<String> = mime_type
        .split("codecs=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let has_audio = mime_type.starts_with("audio/")
        || value.get("audioQuality").is_some()
        || codecs.iter().any(|codec| {
            ["mp4a", "opus", "vorbis", "ac-3", "ec-3"]
                .iter()
                .any(|prefix| codec.starts_with(prefix))
        });
    let has_video = mime_type.starts_with("video/");
    if !has_audio && !has_video {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "unsupported format MIME type",
        ));
    }
    Ok(Format {
        id,
        mime_type,
        codecs,
        has_audio,
        has_video,
        bitrate: value["bitrate"].as_u64().unwrap_or(0),
        height: value["height"].as_u64().and_then(|n| u32::try_from(n).ok()),
        content_length: value["contentLength"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| value["contentLength"].as_u64()),
    })
}

pub fn media_info(response: &Value, data: &Value) -> Result<MediaInfo> {
    check_playability(response)?;
    let details = &response["videoDetails"];
    let id = details["videoId"]
        .as_str()
        .filter(|id| id.len() == 11)
        .ok_or_else(|| Error::new(ErrorCode::Protocol, "response lacks video ID"))?
        .to_owned();
    let title = details["title"]
        .as_str()
        .ok_or_else(|| Error::new(ErrorCode::Protocol, "response lacks video title"))?
        .to_owned();
    let duration = details["lengthSeconds"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s > 0.0);
    let mut found = Vec::new();
    collect_key(data, "chapterRenderer", &mut found);
    let mut chapters: Vec<Chapter> = found
        .into_iter()
        .filter_map(|value| {
            Some(Chapter {
                title: text(&value["title"]),
                start: value["timeRangeStartMillis"].as_f64()? / 1000.0,
                end: None,
            })
        })
        .collect();
    if chapters.is_empty() {
        let mut markers = Vec::new();
        collect_key(data, "macroMarkersListItemRenderer", &mut markers);
        chapters = markers
            .into_iter()
            .filter_map(|value| {
                Some(Chapter {
                    title: text(&value["title"]),
                    start: value
                        .pointer("/onTap/watchEndpoint/startTimeSeconds")?
                        .as_f64()?,
                    end: None,
                })
            })
            .collect();
    }
    chapters.sort_by(|a, b| a.start.total_cmp(&b.start));
    chapters.dedup_by(|a, b| (a.start - b.start).abs() < 0.001);
    let has_chapters = !chapters.is_empty();
    let chapters = normalize_chapters(&title, duration, chapters)?;
    let thumbnails = details
        .pointer("/thumbnail/thumbnails")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v["url"].as_str())
        .filter(|s| s.starts_with("https://"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let formats = raw_formats(response)
        .into_iter()
        .map(format)
        .collect::<Result<Vec<_>>>()?;
    if formats.is_empty()
        && response.pointer("/streamingData/hlsManifestUrl").is_none()
        && response.pointer("/streamingData/dashManifestUrl").is_none()
    {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "no supported downloadable formats",
        ));
    }
    let mut warnings = Vec::new();
    if thumbnails.is_empty() {
        warnings.push("cover_missing".into());
    }
    Ok(MediaInfo {
        id,
        title,
        duration,
        chapters,
        has_chapters,
        thumbnails,
        formats,
        warnings,
    })
}

pub fn text(value: &Value) -> String {
    if let Some(s) = value
        .as_str()
        .or_else(|| value["simpleText"].as_str())
        .or_else(|| value["content"].as_str())
    {
        return s.to_owned();
    }
    value["runs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|run| run["text"].as_str())
        .collect()
}

pub fn playlist_title(data: &Value) -> String {
    first_key(data, "playlistMetadataRenderer")
        .map(|v| text(&v["title"]))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Playlist".into())
}

pub fn playlist(data: &Value, title: String, continuation_page: bool) -> Result<PlaylistPage> {
    if data.get("error").is_some() {
        return Err(Error::new(
            ErrorCode::Unavailable,
            "playlist API returned an error",
        ));
    }
    let scope = if continuation_page {
        first_key(data, "appendContinuationItemsAction")
            .or_else(|| first_key(data, "reloadContinuationItemsCommand"))
            .and_then(|value| value.get("continuationItems"))
            .or_else(|| {
                first_key(data, "playlistVideoListContinuation")
                    .and_then(|value| value.get("contents"))
            })
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::Protocol,
                    "playlist continuation response lacks items",
                )
            })?
    } else if let Some(list) = first_key(data, "playlistVideoListRenderer") {
        list.get("contents").unwrap_or(&Value::Null)
    } else {
        // New YouTube playlist layout has lockupViewModel rows in an item section;
        // scope pagination here rather than picking an unrelated section token.
        let mut sections = Vec::new();
        collect_key(data, "itemSectionRenderer", &mut sections);
        sections
            .into_iter()
            .find(|section| {
                section["contents"]
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|row| row.get("lockupViewModel").is_some()))
            })
            .and_then(|section| section.get("contents"))
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::Unavailable,
                    "playlist is unavailable or its layout is unsupported",
                )
            })?
    };
    let rows = scope
        .as_array()
        .ok_or_else(|| Error::new(ErrorCode::Protocol, "playlist items are not an array"))?;
    let mut items = Vec::new();
    let mut continuation = None;
    for row in rows {
        if let Some(renderer) = row.get("playlistVideoRenderer") {
            let id = renderer["videoId"].as_str().unwrap_or("").to_owned();
            let title = text(&renderer["title"]);
            let available = id.len() == 11
                && renderer["isPlayable"].as_bool() != Some(false)
                && !matches!(title.as_str(), "[Private video]" | "[Deleted video]");
            items.push(PlaylistItem {
                id,
                title,
                available,
            });
        } else if let Some(renderer) = row.get("lockupViewModel") {
            if renderer["contentType"].as_str() != Some("LOCKUP_CONTENT_TYPE_VIDEO") {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "non-video item in playlist",
                ));
            }
            let id = renderer["contentId"].as_str().unwrap_or("").to_owned();
            let title = text(&renderer["metadata"]["lockupMetadataViewModel"]["title"]);
            let available =
                id.len() == 11 && !matches!(title.as_str(), "[Private video]" | "[Deleted video]");
            items.push(PlaylistItem {
                id,
                title,
                available,
            });
        } else if let Some(renderer) = row
            .get("continuationItemRenderer")
            .or_else(|| row.get("continuationItemViewModel"))
        {
            continuation = first_key(renderer, "token")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if continuation.is_none() {
                return Err(Error::new(
                    ErrorCode::Protocol,
                    "playlist continuation item lacks token",
                ));
            }
        } else if row.get("messageRenderer").is_none() {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "unrecognized playlist item layout",
            ));
        }
    }
    if items.len() > 1000 {
        return Err(Error::new(
            ErrorCode::Protocol,
            "playlist page exceeds item limit",
        ));
    }
    Ok(PlaylistPage {
        title,
        items,
        complete: continuation.is_none(),
        continuation,
    })
}

pub fn first_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|v| first_key(v, key))),
        Value::Array(values) => values.iter().find_map(|v| first_key(v, key)),
        _ => None,
    }
}
fn collect_key<'a>(value: &'a Value, key: &str, output: &mut Vec<&'a Value>) {
    match value {
        Value::Object(map) => {
            if let Some(v) = map.get(key) {
                output.push(v);
            }
            for v in map.values() {
                collect_key(v, key, output);
            }
        }
        Value::Array(values) => {
            for v in values {
                collect_key(v, key, output);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn inline_json_handles_braces_inside_strings() {
        assert_eq!(
            assigned_json(
                "<script>var ytInitialData = {\"title\":\"}\\\"{\"}; tail</script>",
                "ytInitialData"
            )
            .unwrap()["title"],
            "}\"{"
        );
    }
    #[test]
    fn html_and_javascript_titles_cannot_supply_initial_data() {
        let html = r#"<meta content='ytInitialData = {"fake":true}'>
            <script>var title='ytInitialData = {"fake":true}'; /* ytInitialData = {"fake":true} */
            ytInitialData = demo; var ytInitialData = {"ok":true};</script>"#;
        assert_eq!(
            assigned_json(html, "ytInitialData").unwrap(),
            json!({"ok":true})
        );
        assert!(
            assigned_json(
                r#"<meta content='ytInitialData = {"fake":true}'>"#,
                "ytInitialData"
            )
            .is_err()
        );
        assert_eq!(
            assigned_json(
                r#"<script>window["ytInitialData"] = {"ok":true};</script>"#,
                "ytInitialData"
            )
            .unwrap(),
            json!({"ok":true})
        );
    }
    #[test]
    fn playlist_preserves_duplicates_unavailable_and_continuation() {
        let data = json!({"playlistVideoListRenderer":{"contents":[
            {"playlistVideoRenderer":{"videoId":"FZ8BxMU3BYc","title":{"simpleText":"A"}}},
            {"playlistVideoRenderer":{"videoId":"FZ8BxMU3BYc","title":{"simpleText":"A"}}},
            {"playlistVideoRenderer":{"videoId":"M4gD1WSo5mA","isPlayable":false,"title":{"simpleText":"[Private video]"}}},
            {"continuationItemRenderer":{"continuationEndpoint":{"continuationCommand":{"token":"next"}}}}
        ]}});
        let page = playlist(&data, "test".into(), false).unwrap();
        assert_eq!(page.items.len(), 3);
        assert!(!page.items[2].available);
        assert!(!page.complete);
        assert_eq!(page.continuation.as_deref(), Some("next"));
        assert!(playlist(&json!({"error":{"code":503}}), "test".into(), true).is_err());
    }
    #[test]
    fn modern_playlist_uses_inner_token() {
        let data = json!({"sectionListRenderer":{"contents":[
            {"itemSectionRenderer":{"contents":[
                {"lockupViewModel":{"contentId":"FZ8BxMU3BYc","contentType":"LOCKUP_CONTENT_TYPE_VIDEO","metadata":{"lockupMetadataViewModel":{"title":{"content":"A"}}}}},
                {"continuationItemViewModel":{"continuationCommand":{"innertubeCommand":{"continuationCommand":{"token":"inner"}}}}}
            ]}}, {"continuationItemViewModel":{"continuationCommand":{"token":"outer"}}}
        ]}});
        let page = playlist(&data, "test".into(), false).unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.continuation.as_deref(), Some("inner"));
    }
    #[test]
    fn format_distinguishes_separate_and_muxed_streams() {
        let audio =
            format(&json!({"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\""})).unwrap();
        assert!(audio.has_audio && !audio.has_video);
        let muxed =
            format(&json!({"itag":18,"mimeType":"video/mp4; codecs=\"avc1.42001E, mp4a.40.2\""}))
                .unwrap();
        assert!(muxed.has_audio && muxed.has_video);
    }
    #[test]
    fn private_and_live_are_explicit_errors() {
        assert!(
            check_playability(&json!({"playabilityStatus":{"status":"LOGIN_REQUIRED"}})).is_err()
        );
        assert!(
            check_playability(
                &json!({"playabilityStatus":{"status":"OK"},"videoDetails":{"isLiveContent":true}})
            )
            .is_err()
        );
    }
    #[test]
    fn manifest_only_vod_keeps_metadata_for_hls_resolution() {
        let mut response = json!({"playabilityStatus":{"status":"OK"},
            "videoDetails":{"videoId":"M4gD1WSo5mA","title":"Manifest VOD","lengthSeconds":"10"},
            "streamingData":{"hlsManifestUrl":"https://manifest.googlevideo.com/api/manifest/hls/test"}});
        let info = media_info(&response, &Value::Null).unwrap();
        assert_eq!(info.title, "Manifest VOD");
        assert!(info.formats.is_empty());
        assert_eq!(info.duration, Some(10.0));
        response["streamingData"] =
            json!({"dashManifestUrl":"https://manifest.googlevideo.com/api/manifest/dash/test"});
        assert!(
            media_info(&response, &Value::Null)
                .unwrap()
                .formats
                .is_empty()
        );
        response["streamingData"] = json!({});
        assert!(media_info(&response, &Value::Null).is_err());
    }
    #[test]
    fn chapters_keep_fractional_boundaries_and_single_track_fallback() {
        let response = json!({"playabilityStatus":{"status":"OK"},
            "videoDetails":{"videoId":"M4gD1WSo5mA","title":"A","lengthSeconds":"10"},
            "streamingData":{"adaptiveFormats":[{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\""}]}});
        let data = json!({"markers":[
            {"chapterRenderer":{"title":{"simpleText":"first"},"timeRangeStartMillis":0}},
            {"chapterRenderer":{"title":{"simpleText":"second"},"timeRangeStartMillis":1321}}
        ]});
        let info = media_info(&response, &data).unwrap();
        assert!(info.has_chapters);
        assert_eq!(info.chapters[0].end, Some(1.321));
        assert_eq!(info.chapters[1].start, 1.321);
        assert_eq!(info.chapters[1].end, Some(10.0));
        let info = media_info(&response, &Value::Null).unwrap();
        assert!(!info.has_chapters);
        assert_eq!(info.chapters.len(), 1);
        assert_eq!(info.chapters[0].end, Some(10.0));
    }
}
