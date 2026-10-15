//! Bounded clear VOD HLS subset (RFC 8216). Manifests never reach FFmpeg.
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tuneka_core::*;
use url::Url;

pub struct Variant {
    pub format: Format,
    pub url: Url,
}
fn invalid(message: &str) -> Error {
    Error::new(ErrorCode::Protocol, message)
}
fn unsupported(message: &str) -> Error {
    Error::new(ErrorCode::Unsupported, message)
}

fn lines(text: &str) -> Result<Vec<&str>> {
    if text.len() > 4 * 1024 * 1024 || text.lines().next().is_none_or(|s| s.trim() != "#EXTM3U") {
        return Err(invalid("invalid or oversized HLS manifest"));
    }
    let lines: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if lines.len() > 100_000 || lines.iter().any(|s| s.len() > 65536) {
        return Err(invalid("HLS manifest limit exceeded"));
    }
    Ok(lines)
}
fn attributes(input: &str) -> Result<BTreeMap<&str, &str>> {
    let mut result = BTreeMap::new();
    let mut rest = input;
    while !rest.is_empty() {
        let (key, value) = rest
            .split_once('=')
            .ok_or_else(|| invalid("invalid HLS attribute"))?;
        if key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(invalid("invalid HLS attribute name"));
        }
        let (value, remaining) = if let Some(quoted) = value.strip_prefix('"') {
            let end = quoted
                .find('"')
                .ok_or_else(|| invalid("unterminated HLS attribute"))?;
            (&quoted[..end], &quoted[end + 1..])
        } else {
            let end = value.find(',').unwrap_or(value.len());
            (&value[..end], &value[end..])
        };
        if result.insert(key, value).is_some() {
            return Err(invalid("duplicate HLS attribute"));
        }
        rest = if remaining.is_empty() {
            ""
        } else {
            remaining
                .strip_prefix(',')
                .ok_or_else(|| invalid("invalid HLS attribute separator"))?
        };
    }
    Ok(result)
}
fn remote(base: &Url, value: &str) -> Result<Url> {
    if value.len() > 65536 {
        return Err(invalid("HLS URL limit exceeded"));
    }
    let url = base.join(value).map_err(|_| invalid("invalid HLS URL"))?;
    if !super::valid_remote(&url, true) {
        return Err(invalid("HLS URL host is not allowed"));
    }
    Ok(url)
}
fn field<'a>(url: &'a Url, key: &str) -> Option<&'a str> {
    let parts: Vec<_> = url.path().split('/').collect();
    parts.windows(2).find(|p| p[0] == key).map(|p| p[1])
}
fn itag(url: &Url) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == "itag")
        .map(|(_, v)| v.into_owned())
        .or_else(|| field(url, "itag").map(str::to_owned))
        .filter(|v| !v.is_empty() && v.len() <= 10 && v.bytes().all(|b| b.is_ascii_digit()))
}

pub fn variants(base: &Url, text: &str) -> Result<Vec<Variant>> {
    let lines = lines(text)?;
    if lines.iter().any(|s| s.starts_with("#EXT-X-SESSION-KEY:")) {
        return Err(unsupported("encrypted HLS is unsupported"));
    }
    let rendition_rows: Vec<_> = lines
        .iter()
        .filter_map(|s| s.strip_prefix("#EXT-X-MEDIA:"))
        .collect();
    if rendition_rows.len() > 128
        || lines
            .iter()
            .filter(|s| s.starts_with("#EXT-X-STREAM-INF:"))
            .count()
            > 128
    {
        return Err(invalid("HLS master row limit exceeded"));
    }
    let renditions = rendition_rows
        .into_iter()
        .map(attributes)
        .collect::<Result<Vec<_>>>()?;
    let mut variants = Vec::new();
    let mut pending = None;
    for line in &lines {
        if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            if pending.is_some() {
                return Err(invalid("HLS variant lacks URI"));
            }
            pending = Some(attributes(attrs)?);
        } else if !line.starts_with('#') {
            let attrs = pending
                .take()
                .ok_or_else(|| invalid("HLS master contains an unexpected URI"))?;
            let url = remote(base, line)?;
            let id = itag(&url)
                .ok_or_else(|| unsupported("HLS variant lacks a stable YouTube format ID"))?;
            let mut codecs: Vec<String> = attrs
                .get("CODECS")
                .ok_or_else(|| unsupported("HLS variant lacks codecs"))?
                .split(',')
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
                .collect();
            let mut has_audio = codecs.iter().any(|c| {
                ["mp4a", "ac-3", "ec-3", "opus"]
                    .iter()
                    .any(|p| c.starts_with(p))
            });
            let has_video = codecs.iter().any(|c| {
                ["avc", "hvc", "hev", "vp", "av01"]
                    .iter()
                    .any(|p| c.starts_with(p))
            });
            if !has_audio && !has_video {
                continue;
            }
            if let Some(group) = attrs.get("AUDIO") {
                if !renditions
                    .iter()
                    .any(|a| a.get("TYPE") == Some(&"AUDIO") && a.get("GROUP-ID") == Some(group))
                {
                    return Err(invalid("HLS variant references a missing audio group"));
                }
                let matching: Vec<_> = renditions
                    .iter()
                    .filter(|a| {
                        a.get("TYPE") == Some(&"AUDIO")
                            && a.get("GROUP-ID") == Some(group)
                            && a.contains_key("URI")
                    })
                    .collect();
                if !matching.is_empty() {
                    let preferred: Vec<_> = matching
                        .iter()
                        .copied()
                        .filter(|a| a.get("DEFAULT") == Some(&"YES"))
                        .collect();
                    let rendition = match preferred.as_slice() {
                        [one] => *one,
                        [] if matching.len() == 1 => matching[0],
                        _ => return Err(unsupported("ambiguous HLS audio rendition group")),
                    };
                    let audio_url = remote(base, rendition["URI"])?;
                    let audio_id = itag(&audio_url)
                        .ok_or_else(|| unsupported("HLS audio lacks a stable YouTube format ID"))?;
                    let audio_codecs: Vec<_> = codecs
                        .iter()
                        .filter(|c| {
                            ["mp4a", "ac-3", "ec-3", "opus"]
                                .iter()
                                .any(|p| c.starts_with(p))
                        })
                        .cloned()
                        .collect();
                    if audio_codecs.is_empty() {
                        return Err(unsupported("HLS audio rendition lacks codec"));
                    }
                    if let Some(existing) = variants
                        .iter()
                        .find(|v: &&Variant| v.format.id == format!("hls-{audio_id}"))
                    {
                        if existing.format.codecs != audio_codecs
                            || identity_url(&existing.url) != identity_url(&audio_url)
                        {
                            return Err(unsupported("ambiguous HLS audio rendition ID"));
                        }
                    } else {
                        variants.push(Variant {
                            format: Format {
                                id: format!("hls-{audio_id}"),
                                mime_type: "application/vnd.apple.mpegurl".into(),
                                codecs: audio_codecs.clone(),
                                has_audio: true,
                                has_video: false,
                                bitrate: 0,
                                height: None,
                                content_length: None,
                            },
                            url: audio_url,
                        });
                    }
                    codecs.retain(|c| !audio_codecs.contains(c));
                    has_audio = false;
                }
            }
            let height = attrs
                .get("RESOLUTION")
                .and_then(|s| s.split_once('x'))
                .and_then(|(_, h)| h.parse::<u32>().ok());
            let bitrate = attrs
                .get("AVERAGE-BANDWIDTH")
                .or_else(|| attrs.get("BANDWIDTH"))
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| invalid("invalid HLS bandwidth"))?;
            let format = Format {
                id: format!("hls-{id}"),
                mime_type: "application/vnd.apple.mpegurl".into(),
                codecs,
                has_audio,
                has_video,
                bitrate,
                height,
                content_length: None,
            };
            if let Some(existing) = variants
                .iter()
                .find(|v: &&Variant| v.format.id == format.id)
            {
                if existing.format.codecs != format.codecs
                    || existing.format.height != format.height
                {
                    return Err(unsupported("ambiguous HLS rendition ID"));
                }
            } else {
                variants.push(Variant { format, url });
            }
            if variants.len() > 128 {
                return Err(invalid("too many HLS variants"));
            }
        }
    }
    if pending.is_some() {
        return Err(invalid("HLS variant lacks URI"));
    }
    if variants.is_empty() {
        return Err(unsupported("no supported clear HLS variant"));
    }
    Ok(variants)
}

fn byte_range(value: &str, previous: Option<(u64, &str)>, uri: &str) -> Result<(u64, u64)> {
    let (length, offset) = value
        .split_once('@')
        .map_or((value, None), |(l, o)| (l, Some(o)));
    let length: u64 = length
        .parse()
        .map_err(|_| invalid("invalid HLS range length"))?;
    let start = match offset {
        Some(o) => o.parse().map_err(|_| invalid("invalid HLS range offset"))?,
        None => previous
            .filter(|(_, old)| *old == uri)
            .map(|(next, _)| next)
            .ok_or_else(|| invalid("implicit HLS range lacks matching predecessor"))?,
    };
    let end = start
        .checked_add(length)
        .and_then(|n| n.checked_sub(1))
        .filter(|_| length > 0)
        .ok_or_else(|| invalid("invalid HLS range bounds"))?;
    Ok((start, end))
}
pub(super) fn identity_url(url: &Url) -> String {
    // Keep resource selectors (including unknown ones) and remove only known
    // authentication/routing fields. Unknown volatile fields fail closed on resume.
    let volatile = |key: &str| {
        matches!(
            key,
            "expire"
                | "ei"
                | "ip"
                | "ipbits"
                | "sparams"
                | "lsparams"
                | "sig"
                | "lsig"
                | "signature"
                | "spc"
                | "n"
                | "pot"
                | "po"
                | "c"
                | "cver"
                | "cbr"
                | "cbrver"
                | "cos"
                | "cosver"
                | "mn"
                | "mm"
                | "mh"
                | "ms"
                | "mt"
                | "mv"
                | "mvi"
                | "pl"
                | "rms"
                | "initcwndbps"
                | "fvip"
                | "keepalive"
                | "fexp"
                | "alr"
                | "cpn"
                | "bui"
                | "cps"
                | "met"
                | "epbp"
        )
    };
    let mut fields = Vec::new();
    if let Some(path) = url.path().strip_prefix("/videoplayback/") {
        let parts: Vec<_> = path.split('/').collect();
        for pair in parts.chunks(2) {
            if pair.len() == 2 && !volatile(pair[0]) {
                fields.push((pair[0].to_owned(), pair[1].to_owned()));
            } else if pair.len() == 1 {
                fields.push(("resource-tail".into(), pair[0].to_owned()));
            }
        }
    } else {
        fields.push(("resource-path".into(), url.path().to_owned()));
    }
    for (key, value) in url.query_pairs() {
        if !volatile(&key) {
            fields.push((key.into_owned(), value.into_owned()));
        }
    }
    fields.sort();
    // JSON escaping avoids delimiter collisions in arbitrary resource selectors.
    serde_json::to_string(&fields).expect("string pairs serialize")
}

pub fn media(base: &Url, text: &str, media_id: &str, format: &Format) -> Result<SegmentedStream> {
    let lines = lines(text)?;
    if !lines.contains(&"#EXT-X-ENDLIST") {
        return Err(unsupported("open-ended/live HLS is unsupported"));
    }
    let mut sequence = 0u64;
    let mut durations = Vec::new();
    let mut duration = None;
    let mut range = None;
    let mut previous_range: Option<(u64, String)> = None;
    let mut segments = Vec::new();
    let mut saw_media = false;
    let mut has_map = false;
    for line in lines {
        if line.starts_with("#EXT-X-DISCONTINUITY")
            || line.starts_with("#EXT-X-GAP")
            || line.starts_with("#EXT-X-PART")
            || line.starts_with("#EXT-X-I-FRAMES-ONLY")
        {
            return Err(unsupported(
                "discontinuous, gapped or partial HLS is unsupported",
            ));
        }
        if let Some(value) = line.strip_prefix("#EXT-X-KEY:") {
            if attributes(value)?.get("METHOD") != Some(&"NONE") {
                return Err(unsupported("encrypted HLS is unsupported"));
            }
        } else if line.starts_with("#EXT-X-STREAM-INF:") {
            return Err(invalid("expected HLS media playlist"));
        } else if let Some(value) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            if saw_media {
                return Err(invalid("HLS sequence after media"));
            }
            sequence = value.parse().map_err(|_| invalid("invalid HLS sequence"))?;
        } else if let Some(value) = line.strip_prefix("#EXTINF:") {
            if duration.is_some() {
                return Err(invalid("HLS duration lacks segment"));
            }
            let value = value.split(',').next().unwrap_or("");
            let seconds: f64 = value.parse().map_err(|_| invalid("invalid HLS duration"))?;
            if !seconds.is_finite() || seconds <= 0.0 || seconds > 3600.0 {
                return Err(invalid("invalid HLS duration"));
            }
            duration = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            if range.replace(value.to_owned()).is_some() {
                return Err(invalid("duplicate HLS byte range"));
            }
        } else if let Some(value) = line.strip_prefix("#EXT-X-MAP:") {
            if saw_media || has_map {
                return Err(unsupported(
                    "changing HLS initialization segments are unsupported",
                ));
            }
            let attrs = attributes(value)?;
            let uri = attrs
                .get("URI")
                .ok_or_else(|| invalid("HLS map lacks URI"))?;
            let url = remote(base, uri)?;
            let byte_range = attrs
                .get("BYTERANGE")
                .map(|r| byte_range(r, None, url.as_str()))
                .transpose()?;
            segments.push(MediaSegment {
                url: url.into(),
                byte_range,
            });
            durations.push("init".into());
            has_map = true;
        } else if !line.starts_with('#') {
            let d = duration
                .take()
                .ok_or_else(|| invalid("HLS segment lacks duration"))?;
            let url = remote(base, line)?;
            let byte_range = range
                .take()
                .map(|r| {
                    byte_range(
                        &r,
                        previous_range.as_ref().map(|(n, u)| (*n, u.as_str())),
                        url.as_str(),
                    )
                })
                .transpose()?;
            previous_range =
                byte_range.and_then(|(_, end)| end.checked_add(1).map(|n| (n, url.to_string())));
            segments.push(MediaSegment {
                url: url.into(),
                byte_range,
            });
            durations.push(d);
            saw_media = true;
            if segments.len() > 20_000 {
                return Err(invalid("HLS segment count limit exceeded"));
            }
        }
    }
    if !saw_media || duration.is_some() || range.is_some() {
        return Err(invalid("empty or incomplete HLS playlist"));
    }
    let mut hash = Sha256::new();
    hash.update(format!(
        "hls-v1\n{media_id}\n{}\n{:?}\n{sequence}\n",
        format.id, format.codecs
    ));
    for (segment, duration) in segments.iter().zip(durations) {
        let url = Url::parse(&segment.url).map_err(|_| invalid("invalid segment URL"))?;
        hash.update(format!(
            "{}\n{:?}\n{duration}\n",
            identity_url(&url),
            segment.byte_range
        ));
    }
    Ok(SegmentedStream {
        identity: format!("{:x}", hash.finalize()),
        segments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn base() -> Url {
        Url::parse("https://manifest.googlevideo.com/api/manifest/hls/itag/95/index.m3u8").unwrap()
    }
    fn format() -> Format {
        Format {
            id: "hls-95".into(),
            mime_type: "video/mp2t".into(),
            codecs: vec!["avc1.4d401f".into(), "mp4a.40.2".into()],
            has_audio: true,
            has_video: true,
            bitrate: 1000,
            height: Some(720),
            content_length: None,
        }
    }
    #[test]
    fn master_quoted_codecs_relative_urls_and_transport_id() {
        let variants=variants(&base(),"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000,CODECS=\"avc1.4d401f,mp4a.40.2\",RESOLUTION=1280x720\nhttps://rr1.googlevideo.com/manifest/itag/95/index.m3u8\n").unwrap();
        assert_eq!(variants[0].format.id, "hls-95");
        assert!(variants[0].format.has_audio);
        assert_eq!(variants[0].format.height, Some(720));
    }
    #[test]
    fn finite_plan_init_and_implicit_byte_ranges() {
        let text = "#EXTM3U\n#EXT-X-MAP:URI=\"file.mp4\",BYTERANGE=\"100@0\"\n#EXTINF:2.5,\n#EXT-X-BYTERANGE:200@100\nfile.mp4\n#EXTINF:1.0,\n#EXT-X-BYTERANGE:50\nfile.mp4\n#EXT-X-ENDLIST\n";
        let plan = media(&base(), text, "abcdefghijk", &format()).unwrap();
        assert_eq!(plan.segments.len(), 3);
        assert_eq!(plan.segments[2].byte_range, Some((300, 349)));
    }
    #[test]
    fn refresh_keeps_identity_but_segment_or_timeline_changes_do_not() {
        let make = |sig: &str, sq: u32, duration: f64| {
            format!(
                "#EXTM3U\n#EXTINF:{duration},\nhttps://rr1.googlevideo.com/videoplayback/id/video/itag/95/sq/{sq}/sig/{sig}\n#EXT-X-ENDLIST\n"
            )
        };
        let id = |s: String| {
            media(&base(), &s, "abcdefghijk", &format())
                .unwrap()
                .identity
        };
        assert_eq!(id(make("old", 1, 2.0)), id(make("new", 1, 2.0)));
        assert_ne!(id(make("new", 1, 2.0)), id(make("new", 2, 2.0)));
        assert_ne!(id(make("new", 1, 2.0)), id(make("new", 1, 3.0)));
    }
    #[test]
    fn unsafe_or_unsupported_manifests_fail_closed() {
        for s in [
            "#EXTM3U\n#EXTINF:1,\nseg.ts\n",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key\"\n#EXTINF:1,\nseg.ts\n#EXT-X-ENDLIST",
            "#EXTM3U\n#EXT-X-DISCONTINUITY\n#EXTINF:1,\nseg.ts\n#EXT-X-ENDLIST",
            "#EXTM3U\n#EXTINF:1,\nhttps://localhost/secret\n#EXT-X-ENDLIST",
            "#EXTM3U\n#EXTINF:1,\n#EXT-X-BYTERANGE:1\nfile.mp4\n#EXT-X-ENDLIST",
            "#EXTM3U\n#EXTINF:NaN,\nseg.ts\n#EXT-X-ENDLIST",
            "#EXTM3U\n#EXTINF:1,\n#EXT-X-ENDLIST",
        ] {
            assert!(media(&base(), s, "abcdefghijk", &format()).is_err());
        }
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    fn base() -> Url {
        Url::parse("https://manifest.googlevideo.com/master.m3u8").unwrap()
    }
    fn audio(group: &str, itag: u32) -> String {
        format!(
            "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"{group}\",DEFAULT=YES,URI=\"https://rr1.googlevideo.com/videoplayback/id/video/itag/{itag}\"\n"
        )
    }
    fn variant(group: &str, codec: &str, itag: u32) -> String {
        format!(
            "#EXT-X-STREAM-INF:BANDWIDTH=1000,CODECS=\"avc1.4d401e,{codec}\",RESOLUTION=640x360,AUDIO=\"{group}\"\nhttps://rr1.googlevideo.com/videoplayback/id/video/itag/{itag}\n"
        )
    }
    #[test]
    fn external_audio_is_separate_and_duplicate_video_is_deduplicated() {
        let text = format!(
            "#EXTM3U\n{}{}{}{}",
            audio("233", 233),
            audio("234", 234),
            variant("233", "mp4a.40.5", 229),
            variant("234", "mp4a.40.2", 229)
        );
        let variants = variants(&base(), &text).unwrap();
        assert_eq!(variants.len(), 3);
        let video = variants.iter().find(|v| v.format.id == "hls-229").unwrap();
        assert!(!video.format.has_audio);
        assert_eq!(video.format.codecs, ["avc1.4d401e"]);
        assert!(
            variants
                .iter()
                .find(|v| v.format.id == "hls-234")
                .unwrap()
                .format
                .has_audio
        );
    }
    #[test]
    fn conflicting_audio_or_missing_group_is_rejected() {
        let text = format!(
            "#EXTM3U\n{}{}{}",
            audio("233", 233),
            variant("233", "mp4a.40.2", 229),
            variant("233", "opus", 230)
        );
        assert!(variants(&base(), &text).is_err());
        assert!(
            variants(
                &base(),
                &format!("#EXTM3U\n{}", variant("absent", "mp4a.40.2", 229))
            )
            .is_err()
        );
    }
    #[test]
    fn repeated_master_rows_cannot_bypass_resource_limit() {
        let text = format!(
            "#EXTM3U\n{}{}",
            audio("233", 233),
            variant("233", "mp4a.40.2", 229).repeat(129)
        );
        assert!(variants(&base(), &text).is_err());
    }
    #[test]
    fn stable_init_resource_revision_sequence_and_range_are_identity_bound() {
        let format = Format {
            id: "hls-602".into(),
            mime_type: "application/vnd.apple.mpegurl".into(),
            codecs: vec!["vp09".into()],
            has_audio: false,
            has_video: true,
            bitrate: 1,
            height: Some(144),
            content_length: None,
        };
        let text = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-MAP:URI=\"https://rr1.googlevideo.com/videoplayback/id/video/itag/602?file=init-v1&sig=old\"\n#EXTINF:1,\n#EXT-X-BYTERANGE:100@0\nhttps://rr1.googlevideo.com/videoplayback/id/video/itag/602/sq/0/lmt/123\n#EXT-X-ENDLIST\n";
        let identity = |text: &str| {
            media(&base(), text, "abcdefghijk", &format)
                .unwrap()
                .identity
        };
        let original = identity(text);
        assert_eq!(
            original,
            identity(&text.replace(
                "sig=old",
                "sig=new&bui=visitor-new&cps=2&met=123&epbp=session-new"
            ))
        );
        for (before, after) in [
            ("init-v1", "init-v2"),
            ("lmt/123", "lmt/124"),
            ("SEQUENCE:0", "SEQUENCE:1"),
            ("100@0", "100@1"),
            ("sq/0", "sq/1"),
        ] {
            assert_ne!(original, identity(&text.replace(before, after)));
        }
    }
}
