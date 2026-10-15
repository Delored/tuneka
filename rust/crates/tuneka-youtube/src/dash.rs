//! Bounded static, single-period, clear ISO-BMFF DASH subset.
//! Timing/addressing: https://dashif.org/Guidelines-TimingModel/
//! Flat SIDX offsets: ISO/IEC 14496-12 SegmentIndexBox (DASH-IF IOP).
use roxmltree::{Document, Node, ParsingOptions};
use sha2::{Digest, Sha256};
use tuneka_core::*;
use url::Url;

const MAX_SEGMENTS: usize = 20_000;
const MAX_RANGE: u64 = 128 * 1024 * 1024;
const NS: &str = "urn:mpeg:dash:schema:mpd:2011";

// Deliberately no Debug/Serialize: resolved URLs contain authentication.
pub struct Representation {
    pub format: Format,
    pub addressing: Addressing,
}
pub enum Addressing {
    Segments(SegmentedStream),
    Indexed(Index),
}
pub struct Index {
    pub url: Url,
    pub initialization: (u64, u64),
    pub index_range: (u64, u64),
    shape: String,
    timescale: Option<u64>,
}
fn invalid(s: &str) -> Error {
    Error::new(ErrorCode::Protocol, s)
}
fn unsupported(s: &str) -> Error {
    Error::new(ErrorCode::Unsupported, s)
}
fn tag(n: Node<'_, '_>, name: &str) -> bool {
    n.is_element()
        && n.tag_name().name() == name
        && n.tag_name().namespace().is_none_or(|s| s == NS)
}
fn children<'a, 'b>(n: Node<'a, 'b>, name: &str) -> Vec<Node<'a, 'b>> {
    n.children().filter(|n| tag(*n, name)).collect()
}
fn one<'a, 'b>(n: Node<'a, 'b>, name: &str) -> Result<Option<Node<'a, 'b>>> {
    let nodes = children(n, name);
    if nodes.len() > 1 {
        return Err(unsupported("ambiguous DASH element"));
    }
    Ok(nodes.first().copied())
}
fn number(s: &str) -> Result<u64> {
    s.parse().map_err(|_| invalid("invalid DASH integer"))
}
fn attr_num(n: Node<'_, '_>, key: &str, default: u64) -> Result<u64> {
    n.attribute(key)
        .map(number)
        .transpose()
        .map(|v| v.unwrap_or(default))
}
fn range(s: &str) -> Result<(u64, u64)> {
    let (a, b) = s
        .split_once('-')
        .ok_or_else(|| invalid("invalid DASH byte range"))?;
    let (a, b) = (number(a)?, number(b)?);
    b.checked_sub(a)
        .and_then(|v| v.checked_add(1))
        .filter(|v| *v <= MAX_RANGE)
        .ok_or_else(|| invalid("DASH byte range limit exceeded"))?;
    Ok((a, b))
}
fn remote(base: &Url, value: &str) -> Result<Url> {
    if value.len() > 65536 {
        return Err(invalid("DASH URL limit exceeded"));
    }
    let url = base.join(value).map_err(|_| invalid("invalid DASH URL"))?;
    if !super::valid_remote(&url, true)
        || !url
            .host_str()
            .is_some_and(|h| h.ends_with(".googlevideo.com"))
    {
        return Err(invalid("DASH URL host is not allowed"));
    }
    Ok(url)
}
fn base_url(parent: &Url, node: Node<'_, '_>) -> Result<Url> {
    match one(node, "BaseURL")? {
        Some(n) => remote(
            parent,
            n.text()
                .ok_or_else(|| invalid("empty DASH BaseURL"))?
                .trim(),
        ),
        None => Ok(parent.clone()),
    }
}
fn inherited<'a, 'b>(nodes: &[Node<'a, 'b>], key: &str) -> Option<&'a str> {
    nodes.iter().rev().find_map(|n| n.attribute(key))
}
fn shape(format: &Format, extra: &str) -> String {
    format!(
        "{}|{}|{}|{}|{:?}|{}",
        format.id,
        format.mime_type,
        format.codecs.join(","),
        format.bitrate,
        format.height,
        extra
    )
}
fn plan(
    media_id: &str,
    format: &Format,
    extra: &str,
    segments: Vec<MediaSegment>,
) -> Result<SegmentedStream> {
    if segments.len() < 2 || segments.len() > MAX_SEGMENTS {
        return Err(invalid("DASH segment count limit exceeded"));
    }
    let stable: Vec<_> = segments
        .iter()
        .map(|s| {
            let url = Url::parse(&s.url).expect("validated DASH URL");
            (super::hls::identity_url(&url), s.byte_range)
        })
        .collect();
    let bytes = serde_json::to_vec(&("dash-v1", media_id, shape(format, extra), stable))?;
    Ok(SegmentedStream {
        identity: format!("{:x}", Sha256::digest(bytes)),
        segments,
    })
}
fn initialization(base: &Url, node: Node<'_, '_>) -> Result<MediaSegment> {
    let init = one(node, "Initialization")?
        .ok_or_else(|| unsupported("DASH initialization is required"))?;
    list_resource(base, init, "sourceURL", "range")
}

fn list_resource(
    base: &Url,
    node: Node<'_, '_>,
    url_key: &str,
    range_key: &str,
) -> Result<MediaSegment> {
    let source = node.attribute(url_key).map(str::trim);
    let bounds = node.attribute(range_key).map(range).transpose()?;
    if source.is_some_and(str::is_empty) || (source.is_none() && bounds.is_none()) {
        return Err(invalid(
            "DASH list resource requires a nonempty URL or byte range",
        ));
    }
    Ok(MediaSegment {
        url: remote(base, source.unwrap_or(""))?.into(),
        byte_range: bounds,
    })
}

/// Parses only clear static single-period ISO-BMFF MPDs; performs no I/O.
pub fn representations(base: &Url, text: &str, media_id: &str) -> Result<Vec<Representation>> {
    remote(base, "")?;
    if text.len() > 4 * 1024 * 1024 {
        return Err(invalid("DASH manifest size limit exceeded"));
    }
    let doc = Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: 100_000,
        },
    )
    .map_err(|_| invalid("invalid DASH XML"))?;
    for n in doc.descendants().filter(Node::is_element) {
        if n.ancestors().count() > 32
            || n.attributes().len() > 64
            || n.attributes().any(|a| a.value().len() > 65536)
        {
            return Err(invalid("DASH XML structure limit exceeded"));
        }
        if n.tag_name().name() == "ContentProtection" {
            return Err(unsupported("encrypted DASH is unsupported"));
        }
        if n.attributes()
            .any(|a| a.namespace() == Some("http://www.w3.org/1999/xlink"))
        {
            return Err(unsupported("DASH XLink is unsupported"));
        }
    }
    let mpd = doc.root_element();
    if !tag(mpd, "MPD") {
        return Err(invalid("invalid DASH root"));
    }
    if mpd.attribute("type").unwrap_or("static") != "static" {
        return Err(unsupported("dynamic DASH is unsupported"));
    }
    let periods = children(mpd, "Period");
    if periods.len() != 1 {
        return Err(unsupported("DASH requires exactly one Period"));
    }
    let period = periods[0];
    if period.attribute("start").is_some_and(|s| s != "PT0S") {
        return Err(unsupported("nonzero DASH Period start is unsupported"));
    }
    let mpd_base = base_url(base, mpd)?;
    let period_base = base_url(&mpd_base, period)?;
    let duration = period
        .attribute("duration")
        .or_else(|| mpd.attribute("mediaPresentationDuration"));
    let mut result = Vec::new();
    for adaptation in children(period, "AdaptationSet") {
        let adaptation_base = base_url(&period_base, adaptation)?;
        for repr in children(adaptation, "Representation") {
            if result.len() >= 128 {
                return Err(invalid("DASH representation limit exceeded"));
            }
            let nodes = [mpd, period, adaptation, repr];
            let url = base_url(&adaptation_base, repr)?;
            let id = repr
                .attribute("id")
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= 128
                        && s.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                })
                .ok_or_else(|| invalid("invalid DASH representation ID"))?;
            let mime = inherited(&nodes, "mimeType")
                .ok_or_else(|| unsupported("DASH MIME type is required"))?;
            if !matches!(mime, "audio/mp4" | "video/mp4") {
                return Err(unsupported("DASH container is unsupported"));
            }
            let codecs = inherited(&nodes, "codecs")
                .ok_or_else(|| unsupported("DASH codecs are required"))?
                .split(',')
                .map(|s| s.trim().to_owned())
                .collect::<Vec<_>>();
            if codecs.is_empty() || codecs.iter().any(String::is_empty) {
                return Err(invalid("invalid DASH codecs"));
            }
            let format = Format {
                id: format!("dash-{id}"),
                mime_type: mime.into(),
                codecs,
                has_audio: mime == "audio/mp4",
                has_video: mime == "video/mp4",
                bitrate: inherited(&nodes, "bandwidth")
                    .map(number)
                    .transpose()?
                    .unwrap_or(0),
                height: inherited(&nodes, "height")
                    .map(|s| s.parse().map_err(|_| invalid("invalid DASH height")))
                    .transpose()?,
                content_length: None,
            };
            if result
                .iter()
                .any(|r: &Representation| r.format.id == format.id)
            {
                return Err(unsupported("duplicate DASH representation ID"));
            }
            // Accept inheritance of a complete addressing element. Partial overrides
            // require merging DASH descriptors and are deliberately rejected.
            let mut descriptor = None;
            for node in nodes {
                let found: Vec<_> = node
                    .children()
                    .filter(|n| {
                        tag(*n, "SegmentBase")
                            || tag(*n, "SegmentList")
                            || tag(*n, "SegmentTemplate")
                    })
                    .collect();
                if found.len() > 1 {
                    return Err(invalid("ambiguous DASH addressing"));
                }
                if let Some(n) = found.first() {
                    if descriptor.is_some() {
                        return Err(unsupported("DASH addressing overrides are unsupported"));
                    }
                    descriptor = Some(*n);
                }
            }
            let n = descriptor.ok_or_else(|| unsupported("DASH segment addressing is required"))?;
            let addressing = if tag(n, "SegmentBase") {
                let initialization = initialization(&url, n)?;
                if initialization.url != url.as_str() {
                    return Err(unsupported(
                        "external DASH indexed initialization is unsupported",
                    ));
                }
                let init = initialization
                    .byte_range
                    .ok_or_else(|| unsupported("DASH indexed initialization range is required"))?;
                let index_range = range(
                    n.attribute("indexRange")
                        .ok_or_else(|| unsupported("DASH index range is required"))?,
                )?;
                let mut indexed = indexed(format.clone(), url, init, index_range)?;
                if let Addressing::Indexed(index) = &mut indexed.addressing {
                    index.shape.push_str(&descriptor_shape(n));
                    index.timescale = n.attribute("timescale").map(number).transpose()?;
                }
                indexed.addressing
            } else if tag(n, "SegmentList") {
                if one(n, "SegmentTimeline")?.is_some() {
                    return Err(unsupported("DASH SegmentList timeline is unsupported"));
                }
                let mut segments = vec![initialization(&url, n)?];
                for s in children(n, "SegmentURL") {
                    if segments.len() >= MAX_SEGMENTS {
                        return Err(invalid("DASH segment count limit exceeded"));
                    }
                    if s.attribute("index").is_some() || s.attribute("indexRange").is_some() {
                        return Err(unsupported("DASH per-segment indexes are unsupported"));
                    }
                    segments.push(list_resource(&url, s, "media", "mediaRange")?);
                }
                Addressing::Segments(plan(media_id, &format, &descriptor_shape(n), segments)?)
            } else {
                Addressing::Segments(template(&url, n, media_id, &format, id, duration)?)
            };
            result.push(Representation { format, addressing });
        }
    }
    if result.is_empty() {
        return Err(unsupported("DASH contains no supported representations"));
    }
    Ok(result)
}
fn descriptor_shape(n: Node<'_, '_>) -> String {
    let mut attrs: Vec<_> = n
        .attributes()
        .filter(|a| !matches!(a.name(), "media" | "initialization"))
        .map(|a| (a.name(), a.value()))
        .collect();
    attrs.sort();
    // Includes timing and all unknown descriptor attributes, never URL templates.
    serde_json::to_string(&attrs).expect("attributes serialize")
}
fn expand(value: &str, id: &str, bandwidth: u64, number_value: u64, time: u64) -> Result<String> {
    let mut output = String::new();
    let mut rest = value;
    while let Some(pos) = rest.find('$') {
        output.push_str(&rest[..pos]);
        rest = &rest[pos + 1..];
        if let Some(after) = rest.strip_prefix('$') {
            output.push('$');
            rest = after;
            continue;
        }
        let end = rest
            .find('$')
            .ok_or_else(|| invalid("invalid DASH template"))?;
        let key = &rest[..end];
        match key {
            "RepresentationID" => output.push_str(id),
            "Bandwidth" => output.push_str(&bandwidth.to_string()),
            "Number" => output.push_str(&number_value.to_string()),
            "Time" => output.push_str(&time.to_string()),
            _ => return Err(unsupported("DASH template token is unsupported")),
        }
        rest = &rest[end + 1..];
        if output.len() > 65536 {
            return Err(invalid("DASH template size limit exceeded"));
        }
    }
    output.push_str(rest);
    Ok(output)
}
fn seconds(s: &str) -> Result<f64> {
    // Bounded ISO8601 time-only durations: hours/minutes/seconds, fractional seconds.
    let mut rest = s
        .strip_prefix("PT")
        .ok_or_else(|| unsupported("DASH duration syntax is unsupported"))?;
    let mut total = 0.0;
    let mut last = 0;
    while !rest.is_empty() {
        let i = rest
            .find(|c: char| c.is_ascii_alphabetic())
            .ok_or_else(|| invalid("invalid DASH duration"))?;
        let value: f64 = rest[..i]
            .parse()
            .map_err(|_| invalid("invalid DASH duration"))?;
        let (order, multiplier) = match rest.as_bytes()[i] {
            b'H' => (1, 3600.0),
            b'M' => (2, 60.0),
            b'S' => (3, 1.0),
            _ => return Err(unsupported("DASH duration syntax is unsupported")),
        };
        if order <= last || !value.is_finite() || value < 0.0 {
            return Err(invalid("invalid DASH duration"));
        }
        last = order;
        total += value * multiplier;
        rest = &rest[i + 1..];
    }
    if !total.is_finite() || total <= 0.0 || total > 7.0 * 24.0 * 3600.0 {
        return Err(invalid("DASH duration limit exceeded"));
    }
    Ok(total)
}
fn template(
    base: &Url,
    n: Node<'_, '_>,
    media_id: &str,
    format: &Format,
    id: &str,
    duration: Option<&str>,
) -> Result<SegmentedStream> {
    let media = n
        .attribute("media")
        .ok_or_else(|| invalid("DASH media template is required"))?;
    let init = n
        .attribute("initialization")
        .ok_or_else(|| unsupported("DASH initialization template is required"))?;
    if init.contains("$Time") || init.contains("$Number") {
        return Err(unsupported(
            "DASH initialization cannot use segment addressing tokens",
        ));
    }
    let timescale = attr_num(n, "timescale", 1)?;
    if timescale == 0 || n.attribute("endNumber").is_some() || n.attribute("index").is_some() {
        return Err(unsupported("unsupported DASH template addressing"));
    }
    let start = attr_num(n, "startNumber", 1)?;
    let mut timing = Vec::new();
    if let Some(timeline) = one(n, "SegmentTimeline")? {
        let mut next = 0u64;
        for s in children(timeline, "S") {
            let time = attr_num(s, "t", next)?;
            let d = attr_num(s, "d", 0)?;
            let repeat = attr_num(s, "r", 0)?;
            if d == 0
                || repeat >= MAX_SEGMENTS as u64
                || timing.len() as u64 + repeat + 1 >= MAX_SEGMENTS as u64
                || time < next
            {
                return Err(invalid("invalid or unbounded DASH timeline"));
            }
            for i in 0..=repeat {
                let t = time
                    .checked_add(
                        i.checked_mul(d)
                            .ok_or_else(|| invalid("DASH timeline overflow"))?,
                    )
                    .ok_or_else(|| invalid("DASH timeline overflow"))?;
                timing.push((t, d));
            }
            next = time
                .checked_add(
                    (repeat + 1)
                        .checked_mul(d)
                        .ok_or_else(|| invalid("DASH timeline overflow"))?,
                )
                .ok_or_else(|| invalid("DASH timeline overflow"))?;
        }
    } else {
        let d = attr_num(n, "duration", 0)?;
        if d == 0 {
            return Err(invalid("DASH template duration is required"));
        }
        let total =
            seconds(duration.ok_or_else(|| unsupported("finite DASH duration is required"))?)?;
        let count = (total * timescale as f64 / d as f64).ceil();
        if count < 1.0 || count >= MAX_SEGMENTS as f64 {
            return Err(invalid("DASH segment count limit exceeded"));
        }
        for i in 0..count as u64 {
            timing.push((
                i.checked_mul(d)
                    .ok_or_else(|| invalid("DASH timeline overflow"))?,
                d,
            ));
        }
    }
    if timing.is_empty() {
        return Err(invalid("empty DASH timeline"));
    }
    let mut segments = vec![MediaSegment {
        url: remote(base, &expand(init, id, format.bitrate, start, 0)?)?.into(),
        byte_range: None,
    }];
    for (i, (time, _)) in timing.iter().enumerate() {
        let number = start
            .checked_add(i as u64)
            .ok_or_else(|| invalid("DASH segment number overflow"))?;
        segments.push(MediaSegment {
            url: remote(base, &expand(media, id, format.bitrate, number, *time)?)?.into(),
            byte_range: None,
        });
    }
    plan(
        media_id,
        format,
        &format!("{}|{:?}", descriptor_shape(n), timing),
        segments,
    )
}

/// Builds indexed addressing from genuine provider initialization/index metadata.
/// Requires a contiguous file prefix so retaining SIDX preserves its byte offsets.
pub fn indexed(
    format: Format,
    url: Url,
    initialization: (u64, u64),
    index_range: (u64, u64),
) -> Result<Representation> {
    remote(&url, "")?;
    range(&format!("{}-{}", initialization.0, initialization.1))?;
    range(&format!("{}-{}", index_range.0, index_range.1))?;
    if initialization.0 != 0
        || initialization.1.checked_add(1) != Some(index_range.0)
        || index_range.1 - index_range.0 + 1 > 1024 * 1024
    {
        return Err(unsupported(
            "DASH index requires a bounded contiguous initialization prefix",
        ));
    }
    let extra = shape(&format, &format!("base:{index_range:?}"));
    Ok(Representation {
        format,
        addressing: Addressing::Indexed(Index {
            url,
            initialization,
            index_range,
            shape: extra,
            timescale: None,
        }),
    })
}

// Only this observed anonymous DASH session selector is normalized. A plain
// resource ID (including hexadecimal IDs), absent anchors or duplicate anchors
// remains bound to identity. Download URLs themselves are never changed.
fn indexed_identity_url(url: &Url, media_id: &str, format: &Format) -> Url {
    let fields: Vec<_> = url.query_pairs().collect();
    let unique = |name: &str| {
        let mut values = fields.iter().filter(|(key, _)| key == name);
        let first = values.next().map(|(_, value)| value.as_ref());
        if values.next().is_some() { None } else { first }
    };
    let Some(raw_id) = format
        .id
        .strip_prefix("dash-")
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
    else {
        return url.clone();
    };
    let authoritative = media_id.len() == 11
        && media_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    let opaque = unique("id").is_some_and(|id| {
        id.starts_with("o-")
            && id.len() >= 32
            && id.len() <= 256
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    });
    let numeric_lmt = unique("lmt").is_some_and(|value| {
        !value.is_empty()
            && value.bytes().all(|b| b.is_ascii_digit())
            && value.parse::<u64>().is_ok()
    });
    let length_matches = unique("clen")
        .and_then(|value| value.parse::<u64>().ok())
        .zip(format.content_length)
        .is_some_and(|(actual, expected)| actual == expected && actual > 0);
    if !authoritative
        || url.path() != "/videoplayback"
        || !opaque
        || !numeric_lmt
        || !length_matches
        || unique("itag") != Some(raw_id)
    {
        return url.clone();
    }
    let mut stable = url.clone();
    stable.set_query(None);
    stable.query_pairs_mut().extend_pairs(
        fields
            .iter()
            .filter(|(key, _)| key != "id")
            .map(|(key, value)| (key.as_ref(), value.as_ref())),
    );
    stable
}

/// Expands a single flat SIDX response fetched with the exact `index_range`.
/// `first_offset` is relative to the end of the SIDX box, not range start.
pub fn indexed_plan(
    repr: &Representation,
    media_id: &str,
    bytes: &[u8],
) -> Result<SegmentedStream> {
    let Addressing::Indexed(index) = &repr.addressing else {
        return Err(invalid("DASH representation is not indexed"));
    };
    if bytes.len() as u64 != index.index_range.1 - index.index_range.0 + 1 {
        return Err(invalid("DASH index response length mismatch"));
    }
    let read32 = |offset: usize| -> Result<u32> {
        let b = bytes
            .get(offset..offset + 4)
            .ok_or_else(|| invalid("truncated DASH index"))?;
        Ok(u32::from_be_bytes(b.try_into().unwrap()))
    };
    let read64 = |offset: usize| -> Result<u64> {
        let b = bytes
            .get(offset..offset + 8)
            .ok_or_else(|| invalid("truncated DASH index"))?;
        Ok(u64::from_be_bytes(b.try_into().unwrap()))
    };
    if bytes.get(4..8) != Some(b"sidx") {
        return Err(unsupported("DASH index must be a single SIDX box"));
    }
    let (size, header) = match read32(0)? {
        1 => (read64(8)?, 16usize),
        0 => return Err(invalid("unbounded DASH SIDX box")),
        v => (v as u64, 8usize),
    };
    if size != bytes.len() as u64 {
        return Err(invalid("DASH SIDX box length mismatch"));
    }
    let version = *bytes
        .get(header)
        .ok_or_else(|| invalid("truncated DASH index"))?;
    if !matches!(version, 0 | 1)
        || bytes.get(header + 1..header + 4) != Some(&[0, 0, 0])
        || read32(header + 8)? == 0
    {
        return Err(invalid("invalid DASH SIDX header"));
    }
    let (earliest, offset, tail) = if version == 0 {
        (
            read32(header + 12)? as u64,
            read32(header + 16)? as u64,
            header + 20,
        )
    } else {
        (read64(header + 12)?, read64(header + 20)?, header + 28)
    };
    let count_bytes = bytes
        .get(tail..tail + 4)
        .ok_or_else(|| invalid("truncated DASH index"))?;
    if count_bytes[..2] != [0, 0] {
        return Err(invalid("invalid DASH SIDX reserved field"));
    }
    let count = u16::from_be_bytes(count_bytes[2..4].try_into().unwrap()) as usize;
    if count == 0 || count + 1 > MAX_SEGMENTS || tail + 4 + count * 12 != bytes.len() {
        return Err(invalid("invalid DASH SIDX reference count"));
    }
    let mut next = index
        .index_range
        .1
        .checked_add(1)
        .and_then(|v| v.checked_add(offset))
        .ok_or_else(|| invalid("DASH SIDX offset overflow"))?;
    let prefix_end = next
        .checked_sub(1)
        .filter(|end| *end < MAX_RANGE)
        .ok_or_else(|| invalid("DASH indexed prefix limit exceeded"))?;
    let mut segments = vec![MediaSegment {
        url: index.url.to_string(),
        byte_range: Some((0, prefix_end)),
    }];
    if index
        .timescale
        .is_some_and(|value| value != read32(header + 8).unwrap_or(0) as u64)
    {
        return Err(invalid("DASH SIDX timescale does not match manifest"));
    }
    let mut presentation_end = earliest;
    let mut timing = vec![];
    for i in 0..count {
        let pos = tail + 4 + i * 12;
        let reference = read32(pos)?;
        let duration = read32(pos + 4)?;
        if reference >> 31 != 0 {
            return Err(unsupported("hierarchical DASH SIDX is unsupported"));
        }
        presentation_end = presentation_end
            .checked_add(duration as u64)
            .ok_or_else(|| invalid("DASH SIDX timing overflow"))?;
        let length = (reference & 0x7fff_ffff) as u64;
        if length == 0 || length > MAX_RANGE || duration == 0 {
            return Err(invalid("invalid DASH SIDX segment"));
        }
        let end = next
            .checked_add(length - 1)
            .ok_or_else(|| invalid("DASH SIDX range overflow"))?;
        if next <= index.initialization.1 || next <= index.index_range.1 {
            return Err(invalid("overlapping DASH media range"));
        }
        segments.push(MediaSegment {
            url: index.url.to_string(),
            byte_range: Some((next, end)),
        });
        next = end
            .checked_add(1)
            .ok_or_else(|| invalid("DASH SIDX offset overflow"))?;
        timing.push((duration, read32(pos + 8)?));
    }
    if repr
        .format
        .content_length
        .is_some_and(|length| length != next)
    {
        return Err(invalid("DASH SIDX does not cover declared file length"));
    }
    let mut identity_segments = segments.clone();
    let stable: String = indexed_identity_url(&index.url, media_id, &repr.format).into();
    for segment in &mut identity_segments {
        segment.url.clone_from(&stable);
    }
    let mut result = plan(
        media_id,
        &repr.format,
        &format!(
            "{}|{earliest}|{}|{:?}|{:x}",
            index.shape,
            read32(header + 8)?,
            timing,
            Sha256::digest(bytes)
        ),
        identity_segments,
    )?;
    result.segments = segments;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn base() -> Url {
        Url::parse("https://rr1.googlevideo.com/path/manifest.mpd").unwrap()
    }
    fn mpd(body: &str) -> String {
        format!(
            r#"<MPD xmlns="{NS}" type="static" mediaPresentationDuration="PT5S"><Period><AdaptationSet mimeType="audio/mp4" codecs="mp4a.40.2">{body}</AdaptationSet></Period></MPD>"#
        )
    }
    fn list() -> String {
        mpd(
            r#"<Representation id="140" bandwidth="128000"><BaseURL>../audio/</BaseURL><SegmentList timescale="1000" duration="2000"><Initialization sourceURL="init.mp4?sig=old&amp;resource=1"/><SegmentURL media="one.m4s?sig=old"/><SegmentURL media="two.m4s" mediaRange="10-19"/></SegmentList></Representation>"#,
        )
    }
    fn segments(repr: &Representation) -> &SegmentedStream {
        match &repr.addressing {
            Addressing::Segments(s) => s,
            _ => panic!("not segments"),
        }
    }
    #[test]
    fn list_resolves_hierarchy_and_refresh_identity_keeps_unknown_selectors() {
        let original = representations(&base(), &list(), "video").unwrap();
        assert_eq!(original[0].format.id, "dash-140");
        assert!(original[0].format.has_audio);
        let s = segments(&original[0]);
        assert_eq!(
            s.segments[0].url,
            "https://rr1.googlevideo.com/audio/init.mp4?sig=old&resource=1"
        );
        assert_eq!(s.segments[2].byte_range, Some((10, 19)));
        let refresh =
            representations(&base(), &list().replace("sig=old", "sig=new"), "video").unwrap();
        assert_eq!(s.identity, segments(&refresh[0]).identity);
        let changed = representations(
            &base(),
            &list().replace("resource=1", "resource=2"),
            "video",
        )
        .unwrap();
        assert_ne!(s.identity, segments(&changed[0]).identity);
        let changed = representations(&base(), &list(), "other").unwrap();
        assert_ne!(s.identity, segments(&changed[0]).identity);
    }
    #[test]
    fn finite_templates_and_timelines_have_exact_numbers_times_and_bounds() {
        let text = mpd(
            r#"<SegmentTemplate timescale="10" duration="20" startNumber="7" media="$RepresentationID$-$Number$.m4s" initialization="$RepresentationID$-init.mp4"/><Representation id="140"/>"#,
        );
        let result = representations(&base(), &text, "v").unwrap();
        let s = segments(&result[0]);
        assert_eq!(s.segments.len(), 4);
        assert!(s.segments[3].url.ends_with("140-9.m4s"));
        let text = mpd(
            r#"<Representation id="140"><SegmentTemplate timescale="10" media="$Time$.m4s" initialization="init.mp4"><SegmentTimeline><S t="5" d="20" r="1"/><S d="10"/></SegmentTimeline></SegmentTemplate></Representation>"#,
        );
        let result = representations(&base(), &text, "v").unwrap();
        let s = segments(&result[0]);
        assert!(s.segments[1].url.ends_with("5.m4s"));
        assert!(s.segments[3].url.ends_with("45.m4s"));
        for (from, to) in [
            ("r=\"1\"", "r=\"-1\""),
            ("r=\"1\"", "r=\"20000\""),
            ("d=\"20\"", "d=\"0\""),
            ("t=\"5\"", "t=\"18446744073709551615\""),
        ] {
            assert!(representations(&base(), &text.replace(from, to), "v").is_err());
        }
    }
    #[test]
    fn xml_security_hosts_drm_dynamic_and_ambiguity_fail_closed() {
        let input = list();
        for text in [
            input.replace("static", "dynamic"),
            input.replace("<SegmentList", "<ContentProtection/><SegmentList"),
            input.replace("../audio/", "https://evil.example/"),
            input.replace("</Period>", "</Period><Period/>"),
            input.replace(
                "<BaseURL>",
                "<BaseURL>https://evil.example/</BaseURL><BaseURL>",
            ),
            format!("<!DOCTYPE MPD [<!ENTITY x SYSTEM 'file:///etc/passwd'>]>{input}"),
            input.replace(
                "id=\"140\"",
                "id=\"140\" xlink:href=\"x\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"",
            ),
        ] {
            assert!(representations(&base(), &text, "v").is_err());
        }
        assert!(representations(&base(), &"x".repeat(4 * 1024 * 1024 + 1), "v").is_err());
        let deep = format!("{}{}{}", "<x>".repeat(35), input, "</x>".repeat(35));
        assert!(representations(&base(), &deep, "v").is_err());
        assert!(representations(&base(), &input.replace("10-19", "20-10"), "v").is_err());
    }
    fn sidx(version: u8, first_offset: u64) -> Vec<u8> {
        let mut b = vec![0; 8];
        b[4..8].copy_from_slice(b"sidx");
        b.extend([version, 0, 0, 0]);
        b.extend(1u32.to_be_bytes());
        b.extend(1000u32.to_be_bytes());
        if version == 0 {
            b.extend(0u32.to_be_bytes());
            b.extend((first_offset as u32).to_be_bytes());
        } else {
            b.extend(0u64.to_be_bytes());
            b.extend(first_offset.to_be_bytes());
        }
        b.extend([0, 0, 0, 2]);
        for length in [100u32, 200] {
            b.extend(length.to_be_bytes());
            b.extend(2000u32.to_be_bytes());
            b.extend(0x90000000u32.to_be_bytes());
        }
        let size = b.len() as u32;
        b[..4].copy_from_slice(&size.to_be_bytes());
        b
    }
    fn indexed_repr(bytes: &[u8], offset: u64) -> Representation {
        let f = Format {
            id: "dash-140".into(),
            mime_type: "audio/mp4".into(),
            codecs: vec!["mp4a.40.2".into()],
            has_audio: true,
            has_video: false,
            bitrate: 128000,
            height: None,
            content_length: Some(10 + bytes.len() as u64 + offset + 300),
        };
        indexed(f, base(), (0, 9), (10, 9 + bytes.len() as u64)).unwrap()
    }
    #[test]
    fn sidx_versions_preserve_prefix_padding_and_exact_original_file_layout() {
        for version in [0, 1] {
            let b = sidx(version, 7);
            let r = indexed_repr(&b, 7);
            let s = indexed_plan(&r, "v", &b).unwrap();
            let first = 10 + b.len() as u64 + 7;
            assert_eq!(
                s.segments
                    .iter()
                    .map(|s| s.byte_range.unwrap())
                    .collect::<Vec<_>>(),
                vec![
                    (0, first - 1),
                    (first, first + 99),
                    (first + 100, first + 299)
                ]
            );
            assert_eq!(s.identity.len(), 64);
        }
    }
    #[test]
    fn sidx_rejects_hierarchy_truncation_overflow_size_and_uncovered_length() {
        let b = sidx(1, 0);
        let r = indexed_repr(&b, 0);
        for offset in [0usize, 4, 8, 36, 38, 40] {
            let mut bad = b.clone();
            bad[offset] ^= 0x80;
            assert!(indexed_plan(&r, "v", &bad).is_err(), "offset{offset}");
        }
        assert!(indexed_plan(&r, "v", &b[..b.len() - 1]).is_err());
        let overflow = sidx(1, u64::MAX);
        let r = indexed_repr(&b, 0);
        assert!(indexed_plan(&r, "v", &overflow).is_err());
        let mut r = indexed_repr(&b, 0);
        r.format.content_length = Some(9999);
        assert!(indexed_plan(&r, "v", &b).is_err());
        let f = r.format.clone();
        assert!(indexed(f.clone(), base(), (1, 9), (10, 65)).is_err());
        assert!(indexed(f, base(), (0, 9), (11, 65)).is_err());
    }
    #[test]
    fn sidx_extended_header_and_identity_changes_are_checked() {
        let original = sidx(0, 0);
        let mut extended = vec![0, 0, 0, 1];
        extended.extend(b"sidx");
        extended.extend((original.len() as u64 + 8).to_be_bytes());
        extended.extend(&original[8..]);
        let r = indexed_repr(&extended, 0);
        assert!(indexed_plan(&r, "v", &extended).is_ok());
        let r = indexed_repr(&original, 0);
        let initial = indexed_plan(&r, "v", &original).unwrap();
        let mut changed = original.clone();
        changed[20..24].copy_from_slice(&1u32.to_be_bytes());
        assert_ne!(
            initial.identity,
            indexed_plan(&r, "v", &changed).unwrap().identity
        );
        let mut zero = original.clone();
        zero[16..20].fill(0);
        assert!(indexed_plan(&r, "v", &zero).is_err());
        let attrs = (0..65).map(|i| format!(" a{i}=\"x\"")).collect::<String>();
        assert!(
            representations(
                &base(),
                &list().replace("<Representation ", &format!("<Representation{attrs} ")),
                "v"
            )
            .is_err()
        );
    }

    #[test]
    fn indexed_session_id_refresh_requires_all_anchors_and_binds_exact_index() {
        let bytes = sidx(0, 0);
        let mut repr = indexed_repr(&bytes, 0);
        let original_id = "o-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ";
        let refreshed_id = "o-ABCDEFGHIJKLMNOPQRSTUVWXYZ9876543210abcdefghij";
        let length = repr.format.content_length.unwrap();
        let source = format!(
            "https://rr1.googlevideo.com/videoplayback?id={original_id}&itag=140&lmt=1700000000000000&clen={length}&unknown=1"
        );
        let fingerprint = |source: &str, index_bytes: &[u8], format: &Format| {
            let r = indexed(
                format.clone(),
                Url::parse(source).unwrap(),
                (0, 9),
                (10, 9 + index_bytes.len() as u64),
            )
            .unwrap();
            indexed_plan(&r, "FZ8BxMU3BYc", index_bytes).unwrap()
        };
        let first = fingerprint(&source, &bytes, &repr.format);
        let refreshed = fingerprint(
            &source.replace(original_id, refreshed_id),
            &bytes,
            &repr.format,
        );
        assert_eq!(first.identity, refreshed.identity);
        assert_eq!(
            refreshed.segments[0].url,
            source.replace(original_id, refreshed_id)
        );
        for (before, after) in [
            ("lmt=1700000000000000", "lmt=1700000000000001"),
            ("unknown=1", "unknown=2"),
        ] {
            assert_ne!(
                first.identity,
                fingerprint(&source.replace(before, after), &bytes, &repr.format).identity
            );
        }
        // SIDX reference_ID does not affect parsed ranges/timing but still binds
        // the exact index, as do version/flags and every other index byte.
        let mut changed = bytes.clone();
        changed[12..16].copy_from_slice(&2u32.to_be_bytes());
        assert_ne!(
            first.identity,
            fingerprint(&source, &changed, &repr.format).identity
        );
        for input in [
            source.replace(original_id, "abcdef0123456789"),
            source.replace("&lmt=1700000000000000", ""),
            source.replace("&itag=140", ""),
            source.replace(&format!("&clen={length}"), ""),
            source.replace("itag=140", "itag=141"),
            source.replace("lmt=1700000000000000", "lmt=bad"),
            format!("{source}&id=duplicate"),
            format!("{source}&lmt=1700000000000000"),
            source.replace("/videoplayback?", "/other?"),
        ] {
            let other = input
                .replace(original_id, refreshed_id)
                .replace("abcdef0123456789", "abcdef0123456780");
            assert_ne!(
                fingerprint(&input, &bytes, &repr.format).identity,
                fingerprint(&other, &bytes, &repr.format).identity,
                "anchors must fail closed"
            );
        }
        let url = Url::parse(&source).unwrap();
        let mut absent_length = repr.format.clone();
        absent_length.content_length = None;
        assert_eq!(
            indexed_identity_url(&url, "FZ8BxMU3BYc", &absent_length),
            url
        );
        repr.format.id = "dash-custom".into();
        assert_eq!(indexed_identity_url(&url, "FZ8BxMU3BYc", &repr.format), url);
        assert_eq!(indexed_identity_url(&url, "missing", &absent_length), url);
    }

    #[test]
    fn segment_list_requires_resource_or_range_and_rejects_empty_attributes() {
        let manifest = |init: &str, media: &str| {
            mpd(&format!(
                r#"<Representation id="140"><BaseURL>file.mp4</BaseURL><SegmentList><Initialization {init}/><SegmentURL {media}/></SegmentList></Representation>"#
            ))
        };
        for (init, media) in [
            ("", r#"media="part.m4s""#),
            (r#"sourceURL="init.mp4""#, ""),
            (r#"sourceURL="""#, r#"media="part.m4s""#),
            (r#"sourceURL="init.mp4""#, r#"media=" ""#),
            (r#"range="""#, r#"media="part.m4s""#),
            (r#"sourceURL="init.mp4""#, r#"mediaRange="""#),
        ] {
            assert!(representations(&base(), &manifest(init, media), "v").is_err());
        }
        for (init, media) in [
            (r#"sourceURL="init.mp4""#, r#"media="part.m4s""#),
            (r#"range="0-9""#, r#"mediaRange="10-19""#),
        ] {
            assert!(representations(&base(), &manifest(init, media), "v").is_ok());
        }
    }

    #[test]
    fn mpd_segment_base_and_separate_video_audio_are_extracted() {
        let text = format!(
            r#"<MPD xmlns="{NS}"><Period><AdaptationSet mimeType="video/mp4" codecs="avc1.4d401e"><Representation id="160" height="144" bandwidth="100000"><BaseURL>video.mp4</BaseURL><SegmentBase indexRange="10-65"><Initialization range="0-9"/></SegmentBase></Representation></AdaptationSet><AdaptationSet mimeType="audio/mp4" codecs="mp4a.40.2"><Representation id="140"><BaseURL>audio.mp4</BaseURL><SegmentBase indexRange="10-65"><Initialization range="0-9"/></SegmentBase></Representation></AdaptationSet></Period></MPD>"#
        );
        let r = representations(&base(), &text, "v").unwrap();
        assert_eq!(r.len(), 2);
        assert!(r[0].format.has_video);
        assert!(!r[0].format.has_audio);
        assert!(r[1].format.has_audio);
        assert_eq!(r[0].format.height, Some(144));
        assert!(matches!(r[0].addressing, Addressing::Indexed(_)));
    }
}
