//! Bounded, independent YouTube metadata and stream extraction.
//! Network response shapes stay inside this adapter; signed URLs are never logged.
mod dash;
mod hls;
mod parse;
mod player;

use async_trait::async_trait;
use reqwest::{Client, Response, redirect::Policy};
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, Semaphore};
use tuneka_core::*;
use url::Url;

const MAX_BODY: usize = 10 * 1024 * 1024;
const PLAYER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";

/// Auto prefers direct streams; explicit modes select supported clear VOD transports.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum TransportPreference {
    #[default]
    Auto,
    Hls,
    Dash,
    Sabr,
}

#[derive(Clone)]
pub struct Youtube {
    transport: TransportPreference,
    client: Client,
    js_workers: Arc<Semaphore>,
    players: Arc<Mutex<HashMap<String, Arc<String>>>>,
}

impl Youtube {
    pub fn new() -> Result<Self> {
        Self::with_transport(TransportPreference::Auto)
    }

    pub fn with_transport(transport: TransportPreference) -> Result<Self> {
        let client = Client::builder()
            .user_agent(PLAYER_AGENT)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(Policy::custom(|attempt| {
                if attempt.previous().len() >= 4 {
                    attempt.error("redirect limit exceeded")
                } else if valid_remote(attempt.url(), true) || valid_thumbnail(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.error("redirect host is not allowed")
                }
            }))
            .build()
            .map_err(|_| Error::new(ErrorCode::Network, "cannot initialize HTTP client"))?;
        Ok(Self {
            transport,
            client,
            js_workers: Arc::new(Semaphore::new(2)),
            players: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    async fn get(&self, url: &str) -> Result<String> {
        let url = Url::parse(url)
            .map_err(|_| Error::new(ErrorCode::InvalidInput, "invalid request URL"))?;
        if !valid_remote(&url, false) {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "request host is not allowed",
            ));
        }
        self.request_body(self.client.get(url))
            .await
            .map(|(_, body)| body)
    }

    async fn request_body(&self, request: reqwest::RequestBuilder) -> Result<(Url, String)> {
        let mut last_error = Error::new(ErrorCode::Network, "YouTube request attempts exhausted");
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(250 << (attempt - 1))).await;
            }
            let request = request.try_clone().ok_or_else(|| {
                Error::new(ErrorCode::Internal, "metadata request cannot be retried")
            })?;
            let response = match request.timeout(Duration::from_secs(10)).send().await {
                Ok(response) => response,
                Err(error) => {
                    let retry = error.is_timeout() || error.is_connect() || error.is_body();
                    last_error = network_error(error);
                    if !retry {
                        return Err(last_error);
                    }
                    continue;
                }
            };
            let status = response.status();
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                last_error = status_error(status);
                continue;
            }
            let effective = response.url().clone();
            match read_body(response).await {
                Ok(body) => return Ok((effective, body)),
                Err(error) if error.code == ErrorCode::Network && status.is_success() => {
                    last_error = error
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error)
    }

    async fn post(&self, method: &str, body: Value, visitor: Option<&str>) -> Result<Value> {
        let mut request = self
            .client
            .post(format!(
                "https://www.youtube.com/youtubei/v1/{method}?prettyPrint=false"
            ))
            .header("Origin", "https://www.youtube.com")
            .json(&body);
        if let Some(visitor) = visitor {
            request = request
                .header("User-Agent", PLAYER_AGENT)
                .header("X-Youtube-Client-Name", "101")
                .header("X-Youtube-Client-Version", "1.02")
                .header("X-Goog-Visitor-Id", visitor);
        }
        let (_, body) = self.request_body(request).await?;
        serde_json::from_str(&body)
            .map_err(|_| Error::new(ErrorCode::Protocol, "invalid YouTube API JSON"))
    }

    async fn watch_player(&self, id: &str) -> Result<(Value, String)> {
        validate_id(id)?;
        let html = self
            .get(&format!("https://www.youtube.com/watch?v={id}&hl=en"))
            .await?;
        let response = parse::assigned_json(&html, "ytInitialPlayerResponse")?;
        parse::check_playability(&response)?;
        Ok((response, html))
    }

    async fn watch(&self, id: &str) -> Result<(Value, Value, String)> {
        let (response, html) = self.watch_player(id).await?;
        let data = parse::assigned_json(&html, "ytInitialData")?;
        Ok((response, data, html))
    }

    async fn native_player(&self, id: &str, html: &str) -> Result<Value> {
        // Public anonymous visitor context is generated by the fresh watch page.
        // No browser session, cookies, account authentication or saved token exists.
        let visitor = parse::json_string_field(html, "visitorData")
            .filter(|value| !value.is_empty() && value.len() <= 4096)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::Protocol,
                    "watch page lacks anonymous visitor context",
                )
            })?;
        let response = self
            .post(
                "player",
                json!({
                    "context": {"client": {"clientName": "VISIONOS", "clientVersion": "1.02",
                        "deviceMake":"Apple", "deviceModel":"RealityDevice17,1",
                        "userAgent":PLAYER_AGENT, "osName":"visionOS", "osVersion":"26.5.23O471",
                        "visitorData":visitor, "hl": "en", "gl": "US"}},
                    "videoId": id, "contentCheckOk": true, "racyCheckOk": true
                }),
                Some(&visitor),
            )
            .await?;
        parse::check_playability(&response)?;
        Ok(response)
    }

    fn sabr_formats(response: &Value) -> Result<Vec<Format>> {
        Ok(Self::sabr_variants(response)?
            .into_iter()
            .map(|(_, format)| format)
            .collect())
    }

    fn sabr_variants(response: &Value) -> Result<Vec<(&Value, Format)>> {
        if response.pointer("/streamingData/serverAbrStreamingUrl").and_then(Value::as_str).is_none()
            || response.pointer("/playerConfig/mediaCommonConfig/mediaUstreamerRequestConfig/videoPlaybackUstreamerConfig").and_then(Value::as_str).is_none() {
            return Err(Error::new(ErrorCode::Unsupported, "SABR session configuration unavailable"));
        }
        let mut formats = Vec::new();
        for raw in parse::raw_formats(response) {
            if !raw["mimeType"]
                .as_str()
                .is_some_and(|m| m.starts_with("video/mp4") || m.starts_with("audio/mp4"))
                || raw["drmFamilies"].as_array().is_some_and(|a| !a.is_empty())
                || raw["isDrc"].as_bool() == Some(true)
            {
                continue;
            }
            if sabr_format(raw).is_err() {
                continue;
            }
            let mut format = parse::format(raw)?;
            if format.has_audio == format.has_video {
                continue;
            }
            format.id = format!("sabr-{}", format.id);
            if formats
                .iter()
                .any(|(_, f): &(&Value, Format)| f.id == format.id)
            {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "ambiguous SABR format IDs",
                ));
            }
            formats.push((raw, format));
            if formats.len() > 256 {
                return Err(Error::new(
                    ErrorCode::Protocol,
                    "SABR format limit exceeded",
                ));
            }
        }
        if formats.is_empty() {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "no supported clear SABR formats",
            ));
        }
        Ok(formats)
    }

    async fn sabr_stream(
        &self,
        response: &Value,
        html: &str,
        media_id: &str,
        format_id: &str,
    ) -> Result<ResolvedStream> {
        use base64::Engine as _;
        let variants = Self::sabr_variants(response)?;
        let formats: Vec<_> = variants.iter().map(|(_, f)| f.clone()).collect();
        let format = formats
            .iter()
            .find(|f| f.id == format_id)
            .cloned()
            .ok_or_else(|| {
                Error::new(ErrorCode::Unavailable, "requested SABR format unavailable")
            })?;
        let selected = variants
            .iter()
            .find(|(_, f)| f.id == format_id)
            .expect("validated SABR selection")
            .0;
        let companion = formats
            .iter()
            .filter(|f| {
                if format.has_audio {
                    f.has_video && !f.has_audio
                } else {
                    f.has_audio && !f.has_video
                }
            })
            .min_by_key(|f| {
                (
                    u8::from(f.has_video && !f.codecs.iter().any(|c| c.starts_with("avc1"))),
                    f.bitrate,
                )
            })
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::Unsupported,
                    "SABR requires audio and video formats",
                )
            })?;
        let companion_raw = variants
            .iter()
            .find(|(_, f)| f.id == companion.id)
            .expect("validated SABR companion")
            .0;
        let (audio, video) = if format.has_audio {
            (sabr_format(selected)?, sabr_format(companion_raw)?)
        } else {
            (sabr_format(companion_raw)?, sabr_format(selected)?)
        };
        let value = response
            .pointer("/streamingData/serverAbrStreamingUrl")
            .and_then(Value::as_str)
            .filter(|v| v.len() <= 65536)
            .ok_or_else(|| Error::new(ErrorCode::Unsupported, "no public SABR URL"))?;
        let url =
            Url::parse(value).map_err(|_| Error::new(ErrorCode::Protocol, "invalid SABR URL"))?;
        let url = self.decipher_url(url, None, html).await?;
        let encoded = response.pointer("/playerConfig/mediaCommonConfig/mediaUstreamerRequestConfig/videoPlaybackUstreamerConfig")
            .and_then(Value::as_str).filter(|s| s.len() <= 65536).ok_or_else(|| Error::new(ErrorCode::Unsupported, "SABR playback configuration unavailable"))?;
        let config = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded.trim_end_matches('='))
            .map_err(|_| Error::new(ErrorCode::Protocol, "invalid SABR playback configuration"))?;
        let context = parse::json_object_field(html, "INNERTUBE_CONTEXT")
            .ok_or_else(|| Error::new(ErrorCode::Protocol, "WEB client context unavailable"))?;
        let client_version = context
            .pointer("/client/clientVersion")
            .and_then(Value::as_str)
            .filter(|v| v.len() <= 128)
            .ok_or_else(|| Error::new(ErrorCode::Protocol, "WEB client version unavailable"))?
            .to_owned();
        let duration_ms = response
            .pointer("/videoDetails/lengthSeconds")
            .and_then(Value::as_str)
            .and_then(|v| v.parse::<u64>().ok())
            .and_then(|n| n.checked_mul(1000))
            .filter(|n| *n > 0)
            .ok_or_else(|| {
                Error::new(ErrorCode::Unsupported, "SABR requires finite VOD duration")
            })?;
        Ok(ResolvedStream {
            media_id: media_id.into(),
            format,
            url: url.into(),
            segmented: None,
            sabr: Some(SabrStream {
                client_version,
                user_agent: PLAYER_AGENT.into(),
                video_playback_config: config,
                po_token: None,
                audio,
                video,
                selected_itag: selected["itag"].as_u64().unwrap() as u32,
                duration_ms,
            }),
        })
    }

    async fn decipher_url(
        &self,
        url: Url,
        cipher: Option<(String, String)>,
        html: &str,
    ) -> Result<Url> {
        if !valid_remote(&url, true) {
            return Err(Error::new(ErrorCode::Protocol, "media host is not allowed"));
        }
        let n = url_n(&url)?;
        if cipher.is_some() || n.is_some() {
            let source = self.player_source(html).await?;
            return self.decipher_with_source(url, cipher, source).await;
        }
        Ok(url)
    }

    /// Resolve a format from an alternate public player response using its exact
    /// matching player source. Callers own response/media binding and freshness.
    /// Challenges use the same bounded executor and URL validation as watch extraction.
    pub async fn resolve_player_format(
        &self,
        media_id: &str,
        raw_format: &Value,
        player_source: Arc<String>,
    ) -> Result<ResolvedStream> {
        validate_id(media_id)?;
        if !parse::has_transport(raw_format)
            || raw_format["drmFamilies"]
                .as_array()
                .is_some_and(|families| !families.is_empty())
        {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "format has no public transport",
            ));
        }
        let format = parse::format(raw_format)?;
        let (url, cipher) = player::stream_url(raw_format)?;
        let url = self
            .decipher_with_source(url, cipher, player_source)
            .await?;
        Ok(ResolvedStream {
            media_id: media_id.into(),
            format,
            url: url.into(),
            segmented: None,
            sabr: None,
        })
    }

    async fn decipher_with_source(
        &self,
        mut url: Url,
        cipher: Option<(String, String)>,
        source: Arc<String>,
    ) -> Result<Url> {
        if !valid_remote(&url, true) {
            return Err(Error::new(ErrorCode::Protocol, "media host is not allowed"));
        }
        let n = url_n(&url)?;
        if let Some((parameter, signature)) = cipher {
            let transformed =
                player::transform(source.clone(), signature, false, self.js_workers.clone())
                    .await?;
            replace_query(&mut url, &parameter, &transformed);
        }
        if let Some((n, path_index)) = n {
            let transformed = player::transform(source, n, true, self.js_workers.clone()).await?;
            if let Some(index) = path_index {
                let mut parts = url.path().split('/').map(str::to_owned).collect::<Vec<_>>();
                parts[index] = transformed;
                url.set_path(&parts.join("/"));
            } else {
                replace_query(&mut url, "n", &transformed);
            }
        }
        Ok(url)
    }

    async fn dash_representations(
        &self,
        response: &Value,
        html: &str,
        media_id: &str,
    ) -> Result<Vec<dash::Representation>> {
        if let Some(value) = response
            .pointer("/streamingData/dashManifestUrl")
            .and_then(Value::as_str)
        {
            let url = Url::parse(value)
                .map_err(|_| Error::new(ErrorCode::Protocol, "invalid manifest URL"))?;
            let url = self.decipher_url(url, None, html).await?;
            let (url, text) = self.manifest(url.as_str()).await?;
            return dash::representations(&url, &text, media_id);
        }
        // YouTube also supplies DASH on-demand addressing directly in player JSON.
        // This consumes real initRange/indexRange, without manufacturing an MPD.
        let mut found = Vec::new();
        for raw in parse::raw_formats(response) {
            if raw["drmFamilies"].as_array().is_some_and(|a| !a.is_empty())
                || !parse::has_transport(raw)
                || !raw["mimeType"]
                    .as_str()
                    .is_some_and(|m| m.starts_with("video/mp4") || m.starts_with("audio/mp4"))
            {
                continue;
            }
            let Some(init) = json_range(&raw["initRange"]) else {
                continue;
            };
            let Some(index) = json_range(&raw["indexRange"]) else {
                continue;
            };
            let mut format = parse::format(raw)?;
            format.id = format!("dash-{}", format.id);
            let (url, cipher) = player::stream_url(raw)?;
            let url = self.decipher_url(url, cipher, html).await?;
            if found
                .iter()
                .any(|r: &dash::Representation| r.format.id == format.id)
            {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "ambiguous DASH format IDs",
                ));
            }
            found.push(dash::indexed(format, url, init, index)?);
            if found.len() > 256 {
                return Err(Error::new(
                    ErrorCode::Protocol,
                    "DASH representation limit exceeded",
                ));
            }
        }
        if found.is_empty() {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "no supported public DASH addressing",
            ));
        }
        Ok(found)
    }

    async fn index_response(
        &self,
        request: reqwest::RequestBuilder,
        bounds: (u64, u64),
        total: Option<u64>,
    ) -> Result<Vec<u8>> {
        let length = bounds
            .1
            .checked_sub(bounds.0)
            .and_then(|n| n.checked_add(1))
            .filter(|n| *n <= 1024 * 1024)
            .ok_or_else(|| Error::new(ErrorCode::Protocol, "DASH index range limit exceeded"))?;
        let mut last = Error::new(ErrorCode::Network, "DASH index attempts exhausted");
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(250 << (attempt - 1))).await;
            }
            let req = request.try_clone().ok_or_else(|| {
                Error::new(ErrorCode::Internal, "index request cannot be retried")
            })?;
            let response = req
                .timeout(Duration::from_secs(10))
                .header("Accept-Encoding", "identity")
                .header("Range", format!("bytes={}-{}", bounds.0, bounds.1))
                .send()
                .await;
            let mut response = match response {
                Ok(r) => r,
                Err(e) if e.is_timeout() || e.is_connect() || e.is_body() => {
                    last = network_error(e);
                    continue;
                }
                Err(e) => return Err(network_error(e)),
            };
            let effective = response.url();
            let media_origin = effective.scheme() == "https"
                && effective
                    .host_str()
                    .is_some_and(|h| h == "googlevideo.com" || h.ends_with(".googlevideo.com"))
                && effective.port_or_known_default() == Some(443);
            #[cfg(test)]
            let media_origin = media_origin
                || (effective.scheme() == "http" && effective.host_str() == Some("127.0.0.1"));
            if !media_origin {
                return Err(Error::new(
                    ErrorCode::Protocol,
                    "DASH index redirected outside media hosts",
                ));
            }
            let status = response.status();
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                last = status_error(status);
                continue;
            }
            if status != reqwest::StatusCode::PARTIAL_CONTENT {
                return Err(if status.is_success() {
                    Error::new(ErrorCode::Protocol, "DASH index requires HTTP 206")
                } else if matches!(status.as_u16(), 401 | 403 | 410) {
                    Error::new(
                        ErrorCode::Unavailable,
                        "DASH index URL expired or was denied",
                    )
                } else {
                    status_error(status)
                });
            }
            let range = response
                .headers()
                .get("Content-Range")
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range);
            if !range.is_some_and(|(start, end, size)| {
                (start, end) == bounds && total.is_none_or(|n| n == size)
            }) || response.content_length().is_some_and(|n| n != length)
                || response
                    .headers()
                    .get("Content-Encoding")
                    .is_some_and(|v| v != "identity")
            {
                return Err(Error::new(
                    ErrorCode::Protocol,
                    "DASH index range response mismatch",
                ));
            }
            let mut bytes = Vec::new();
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        if bytes.len() + chunk.len() > length as usize {
                            return Err(Error::new(
                                ErrorCode::Protocol,
                                "oversized DASH index body",
                            ));
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    Ok(None) => break,
                    Err(_) => {
                        bytes.clear();
                        break;
                    }
                }
            }
            if bytes.len() == length as usize {
                return Ok(bytes);
            }
            last = Error::new(ErrorCode::Network, "truncated DASH index body");
        }
        Err(last)
    }

    async fn hls_variants(&self, response: &Value, html: &str) -> Result<Vec<hls::Variant>> {
        let manifest = response
            .pointer("/streamingData/hlsManifestUrl")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new(ErrorCode::Unsupported, "no public HLS manifest"))?;
        let url = Url::parse(manifest)
            .map_err(|_| Error::new(ErrorCode::Protocol, "invalid manifest URL"))?;
        let url = self.decipher_url(url, None, html).await?;
        let (url, text) = self.manifest(url.as_str()).await?;
        hls::variants(&url, &text)
    }

    async fn manifest(&self, value: &str) -> Result<(Url, String)> {
        let url = Url::parse(value)
            .map_err(|_| Error::new(ErrorCode::Protocol, "invalid manifest URL"))?;
        if !valid_remote(&url, true) {
            return Err(Error::new(
                ErrorCode::Protocol,
                "manifest host is not allowed",
            ));
        }
        self.request_body(self.client.get(url).header("User-Agent", PLAYER_AGENT))
            .await
    }

    async fn player_source(&self, html: &str) -> Result<Arc<String>> {
        let path = parse::json_string_field(html, "jsUrl")
            .or_else(|| parse::json_string_field(html, "PLAYER_JS_URL"))
            .ok_or_else(|| Error::new(ErrorCode::Unsupported, "player script URL was not found"))?;
        let base = Url::parse("https://www.youtube.com").expect("constant URL");
        let url = base
            .join(&path)
            .map_err(|_| Error::new(ErrorCode::Protocol, "invalid player script URL"))?;
        if !valid_remote(&url, false) {
            return Err(Error::new(
                ErrorCode::Protocol,
                "player script host is not allowed",
            ));
        }
        if let Some(source) = self.players.lock().await.get(url.as_str()).cloned() {
            return Ok(source);
        }
        let source = Arc::new(self.get(url.as_str()).await?);
        let mut cache = self.players.lock().await;
        if cache.len() >= 4 {
            cache.clear();
        }
        cache.insert(url.to_string(), source.clone());
        Ok(source)
    }

    /// Explicit live-smoke helper. Reads at most `limit` media bytes (first segment for HLS), never the manifest, without persisting a URL.
    pub async fn probe_stream(&self, stream: &ResolvedStream, limit: usize) -> Result<usize> {
        if stream.sabr.is_some() {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "SABR probes require the session downloader",
            ));
        }
        if !(1..=65536).contains(&limit) {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "probe limit must be 1..65536",
            ));
        }
        let segment = stream
            .segmented
            .as_ref()
            .and_then(|plan| plan.segments.first());
        if stream.segmented.is_some() && segment.is_none() {
            return Err(Error::new(ErrorCode::Protocol, "empty segmented stream"));
        }
        let url = Url::parse(
            segment
                .map(|segment| segment.url.as_str())
                .unwrap_or(&stream.url),
        )
        .map_err(|_| Error::new(ErrorCode::Protocol, "invalid media URL"))?;
        let start = segment
            .and_then(|segment| segment.byte_range)
            .map(|(start, _)| start)
            .unwrap_or(0);
        let end = start
            .checked_add(limit as u64 - 1)
            .ok_or_else(|| Error::new(ErrorCode::Protocol, "probe range overflow"))?
            .min(
                segment
                    .and_then(|segment| segment.byte_range)
                    .map(|(_, end)| end)
                    .unwrap_or(u64::MAX),
            );
        if end < start {
            return Err(Error::new(ErrorCode::Protocol, "invalid probe range"));
        }
        if !valid_remote(&url, true) {
            return Err(Error::new(ErrorCode::Protocol, "media host is not allowed"));
        }
        let mut response = self
            .client
            .get(url)
            .header("User-Agent", PLAYER_AGENT)
            .header("Range", format!("bytes={start}-{end}"))
            .send()
            .await
            .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(status_error(response.status()));
        }
        let mut count = 0;
        while let Some(bytes) = response.chunk().await.map_err(network_error)? {
            count += bytes.len().min(limit - count);
            if count >= limit {
                break;
            }
        }
        if count == 0 {
            return Err(Error::new(ErrorCode::Protocol, "empty media response"));
        }
        Ok(count)
    }
}

#[async_trait]
impl MetadataProvider for Youtube {
    async fn fetch_thumbnail(&self, url: &str, target: PathBuf) -> Result<PathBuf> {
        let url = Url::parse(url)
            .map_err(|_| Error::new(ErrorCode::InvalidInput, "invalid thumbnail URL"))?;
        if !valid_thumbnail(&url) {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "thumbnail host is not allowed",
            ));
        }
        let mut response = self.client.get(url).send().await.map_err(network_error)?;
        if !response.status().is_success() {
            return Err(status_error(response.status()));
        }
        // Recheck the effective URL: the shared client's redirect policy also
        // serves media streams, which are not valid thumbnail origins.
        if !valid_thumbnail(response.url()) {
            return Err(Error::new(
                ErrorCode::Protocol,
                "thumbnail redirected outside allowed image hosts",
            ));
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !matches!(
            content_type.split(';').next(),
            Some("image/jpeg" | "image/png")
        ) {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "thumbnail is not JPEG or PNG",
            ));
        }
        if response
            .content_length()
            .is_some_and(|n| n > 8 * 1024 * 1024)
        {
            return Err(Error::new(
                ErrorCode::Protocol,
                "thumbnail exceeds size limit",
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
                return Err(Error::new(
                    ErrorCode::Protocol,
                    "thumbnail exceeds size limit",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() {
            return Err(Error::new(ErrorCode::Protocol, "empty thumbnail response"));
        }
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .await?;
        if let Err(error) = file.write_all(&bytes).await {
            drop(file);
            let _ = tokio::fs::remove_file(&target).await;
            return Err(error.into());
        }
        file.sync_all().await?;
        Ok(target)
    }

    async fn inspect(&self, url: &str) -> Result<MediaInfo> {
        let YoutubeUrl::Video(id) = parse_url(url)? else {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "inspect expects a video URL",
            ));
        };
        let (web, data, html) = self.watch(&id).await?;
        if self.transport == TransportPreference::Sabr {
            let mut info = parse::media_info(&web, &data)?;
            info.formats = Self::sabr_formats(&web)?;
            return Ok(info);
        }
        // Stable ID prefixes bind resolution to the selected transport/client.
        let response = match self.native_player(&id, &html).await {
            Ok(response) => response,
            Err(error)
                if self.transport == TransportPreference::Auto
                    && web
                        .pointer("/streamingData/serverAbrStreamingUrl")
                        .is_some() =>
            {
                let mut info = parse::media_info(&web, &data)?;
                info.formats = Self::sabr_formats(&web).map_err(|_| error)?;
                return Ok(info);
            }
            Err(error) => return Err(error),
        };
        let mut info = parse::media_info(&response, &data)?;
        info.formats.retain(|format| {
            parse::find_format(&response, &format.id).is_some_and(parse::has_transport)
        });
        if self.transport == TransportPreference::Dash {
            info.formats = self
                .dash_representations(&response, &html, &id)
                .await?
                .into_iter()
                .map(|representation| representation.format)
                .collect();
        } else if self.transport == TransportPreference::Hls || info.formats.is_empty() {
            info.formats = if self.transport == TransportPreference::Hls
                || response.pointer("/streamingData/hlsManifestUrl").is_some()
            {
                self.hls_variants(&response, &html)
                    .await?
                    .into_iter()
                    .map(|variant| variant.format)
                    .collect()
            } else if response.pointer("/streamingData/dashManifestUrl").is_some() {
                self.dash_representations(&response, &html, &id)
                    .await?
                    .into_iter()
                    .map(|representation| representation.format)
                    .collect()
            } else {
                Self::sabr_formats(&web)?
            };
        }
        Ok(info)
    }

    async fn playlist_page(&self, url: &str, continuation: Option<&str>) -> Result<PlaylistPage> {
        let YoutubeUrl::Playlist(id) = parse_url(url)? else {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "playlist_page expects a playlist URL",
            ));
        };
        if continuation.is_some_and(|token| token.is_empty() || token.len() > 32768) {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "invalid continuation token",
            ));
        }
        let html = self
            .get(&format!("https://www.youtube.com/playlist?list={id}&hl=en"))
            .await?;
        let data = parse::assigned_json(&html, "ytInitialData")?;
        let title = parse::playlist_title(&data);
        let response = if let Some(token) = continuation {
            let context =
                parse::json_object_field(&html, "INNERTUBE_CONTEXT").unwrap_or_else(|| {
                    json!({"client":{"clientName":"WEB",
                    "clientVersion":"2.20261013.00.00","hl":"en","gl":"US"}})
                });
            self.post(
                "browse",
                json!({"context": context, "continuation": token}),
                None,
            )
            .await?
        } else {
            data
        };
        parse::playlist(&response, title, continuation.is_some())
    }

    async fn resolve_stream(&self, media_id: &str, format_id: &str) -> Result<ResolvedStream> {
        validate_id(media_id)?;
        let numeric_id = format_id
            .strip_prefix("hls-")
            .or_else(|| format_id.strip_prefix("dash-"))
            .or_else(|| format_id.strip_prefix("sabr-"))
            .unwrap_or(format_id);
        if numeric_id.is_empty()
            || numeric_id.len() > 128
            || !numeric_id
                .bytes()
                .all(|v| v.is_ascii_alphanumeric() || v == b'_' || v == b'-')
        {
            return Err(Error::new(ErrorCode::InvalidInput, "invalid format ID"));
        }
        // Always refresh: cached signed URLs must not survive URL expiration.
        let (web, html) = self.watch_player(media_id).await?;
        if format_id.starts_with("sabr-") {
            return self.sabr_stream(&web, &html, media_id, format_id).await;
        }
        let response = self.native_player(media_id, &html).await?;
        if format_id.starts_with("dash-") {
            let representation = self
                .dash_representations(&response, &html, media_id)
                .await?
                .into_iter()
                .find(|r| r.format.id == format_id)
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::Unavailable,
                        "requested DASH format is unavailable",
                    )
                })?;
            let plan = match &representation.addressing {
                dash::Addressing::Segments(plan) => plan.clone(),
                dash::Addressing::Indexed(index) => {
                    let bytes = self
                        .index_response(
                            self.client
                                .get(index.url.clone())
                                .header("User-Agent", PLAYER_AGENT),
                            index.index_range,
                            representation.format.content_length,
                        )
                        .await?;
                    dash::indexed_plan(&representation, media_id, &bytes)?
                }
            };
            let url = plan
                .segments
                .first()
                .ok_or_else(|| Error::new(ErrorCode::Protocol, "empty DASH plan"))?
                .url
                .clone();
            return Ok(ResolvedStream {
                sabr: None,
                media_id: media_id.into(),
                format: representation.format,
                url,
                segmented: Some(plan),
            });
        }
        if format_id.starts_with("hls-") {
            let variant = self
                .hls_variants(&response, &html)
                .await?
                .into_iter()
                .find(|variant| variant.format.id == format_id)
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::Unavailable,
                        "requested HLS format is unavailable",
                    )
                })?;
            let (url, text) = self.manifest(variant.url.as_str()).await?;
            let segmented = hls::media(&url, &text, media_id, &variant.format)?;
            return Ok(ResolvedStream {
                sabr: None,
                media_id: media_id.into(),
                format: variant.format,
                url: url.into(),
                segmented: Some(segmented),
            });
        }
        let selected = parse::find_format(&response, format_id)
            .filter(|format| parse::has_transport(format))
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::Unavailable,
                    "requested format has no supported public transport",
                )
            })?;
        parse::check_playability(&response)?;
        let format = parse::format(selected)?;
        let (url, cipher) = player::stream_url(selected)?;
        let url = self.decipher_url(url, cipher, &html).await?;
        if !valid_remote(&url, true) {
            return Err(Error::new(
                ErrorCode::Protocol,
                "resolved media host is not allowed",
            ));
        }
        Ok(ResolvedStream {
            sabr: None,
            segmented: None,
            media_id: media_id.into(),
            format,
            url: url.into(),
        })
    }
}

fn url_n(url: &Url) -> Result<Option<(String, Option<usize>)>> {
    let mut found = url
        .query_pairs()
        .filter(|(key, _)| key == "n")
        .map(|(_, value)| (value.into_owned(), None))
        .collect::<Vec<_>>();
    if url.path().starts_with("/api/manifest/") {
        let parts = url.path().split('/').collect::<Vec<_>>();
        for (index, part) in parts.iter().enumerate() {
            if *part == "n" {
                let value = parts
                    .get(index + 1)
                    .ok_or_else(|| Error::new(ErrorCode::Protocol, "missing manifest n value"))?;
                // Current manifest challenges are URL-safe ASCII. Do not rewrite
                // other signed path fields or silently decode malformed escapes.
                if !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "unsupported manifest n encoding",
                    ));
                }
                found.push(((*value).into(), Some(index + 1)));
            }
        }
    }
    if found.len() > 1 {
        return Err(Error::new(ErrorCode::Protocol, "ambiguous URL n challenge"));
    }
    if found
        .first()
        .is_some_and(|(v, _)| v.is_empty() || v.len() > 4096)
    {
        return Err(Error::new(ErrorCode::Protocol, "invalid URL n challenge"));
    }
    Ok(found.pop())
}

fn sabr_format(raw: &Value) -> Result<SabrFormat> {
    let value = |key: &str| {
        raw[key]
            .as_u64()
            .or_else(|| raw[key].as_str().and_then(|v| v.parse::<u64>().ok()))
    };
    let itag = value("itag")
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v > 0)
        .ok_or_else(|| Error::new(ErrorCode::Protocol, "invalid SABR itag"))?;
    let last_modified = value("lastModified").filter(|v| *v > 0).ok_or_else(|| {
        Error::new(
            ErrorCode::Protocol,
            "SABR format lacks modification identity",
        )
    })?;
    let content_length = value("contentLength").filter(|v| *v > 0).ok_or_else(|| {
        Error::new(
            ErrorCode::Unsupported,
            "SABR requires declared content length",
        )
    })?;
    let xtags = raw["xtags"]
        .as_str()
        .filter(|v| !v.is_empty())
        .map(str::to_owned);
    if xtags.as_ref().is_some_and(|s| s.len() > 4096) {
        return Err(Error::new(
            ErrorCode::Protocol,
            "SABR format tags exceed limit",
        ));
    }
    Ok(SabrFormat {
        itag,
        last_modified,
        xtags,
        content_length,
    })
}

fn json_range(value: &Value) -> Option<(u64, u64)> {
    let parse = |key: &str| {
        value[key]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| value[key].as_u64())
    };
    let (start, end) = (parse("start")?, parse("end")?);
    (start <= end).then_some((start, end))
}
fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let (range, size) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let (start, end, size): (u64, u64, u64) =
        (start.parse().ok()?, end.parse().ok()?, size.parse().ok()?);
    (start <= end && end < size).then_some((start, end, size))
}

fn replace_query(url: &mut Url, key: &str, value: &str) {
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != key)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(pairs)
        .append_pair(key, value);
}

fn validate_id(id: &str) -> Result<()> {
    if id.len() != 11
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        return Err(Error::new(ErrorCode::InvalidInput, "invalid video ID"));
    }
    Ok(())
}

fn valid_remote(url: &Url, media: bool) -> bool {
    let host = url.host_str().unwrap_or("");
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none_or(|port| port == 443)
        && (host == "youtube.com"
            || host.ends_with(".youtube.com")
            || (media && host.ends_with(".googlevideo.com")))
}

fn valid_thumbnail(url: &Url) -> bool {
    let host = url.host_str().unwrap_or("");
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none_or(|p| p == 443)
        && ["ytimg.com", "ggpht.com"]
            .iter()
            .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

fn network_error(error: reqwest::Error) -> Error {
    // reqwest Display includes the full request URL, including signed parameters.
    let message = if error.is_timeout() {
        "YouTube HTTP request timed out"
    } else if error.is_redirect() {
        "YouTube HTTP redirect was rejected"
    } else if error.is_connect() {
        "YouTube HTTP connection failed"
    } else {
        "YouTube HTTP request failed"
    };
    Error::new(ErrorCode::Network, message)
}
fn status_error(status: reqwest::StatusCode) -> Error {
    Error::new(
        ErrorCode::Network,
        format!("YouTube returned HTTP {}", status.as_u16()),
    )
}
async fn read_body(mut response: Response) -> Result<String> {
    if !response.status().is_success() {
        return Err(status_error(response.status()));
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BODY as u64)
    {
        return Err(Error::new(
            ErrorCode::Protocol,
            "YouTube response exceeds size limit",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(network_error)? {
        if bytes.len() + chunk.len() > MAX_BODY {
            return Err(Error::new(
                ErrorCode::Protocol,
                "YouTube response exceeds size limit",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes)
        .map_err(|_| Error::new(ErrorCode::Protocol, "invalid YouTube response encoding"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_hosts_are_https_and_explicitly_scoped() {
        for value in [
            "http://www.youtube.com/watch",
            "https://youtube.com.evil.test/watch",
            "https://localhost/",
            "https://user:password@www.youtube.com/watch",
            "https://rr1.googlevideo.com:444/video",
        ] {
            assert!(!valid_remote(&Url::parse(value).unwrap(), true));
        }
        let media = Url::parse("https://rr1.googlevideo.com/video").unwrap();
        assert!(valid_remote(&media, true));
        assert!(!valid_remote(&media, false));
        assert!(!valid_thumbnail(&media));
        assert!(valid_thumbnail(
            &Url::parse("https://i.ytimg.com/vi/test/hq.jpg").unwrap()
        ));
        assert!(!valid_thumbnail(
            &Url::parse("https://i.ytimg.com.evil.test/x").unwrap()
        ));
    }
    #[tokio::test]
    async fn query_and_manifest_n_challenges_use_the_same_bounded_player() {
        let youtube = Youtube::new().unwrap();
        let html = r#"{"jsUrl":"/s/player/test/base.js"}"#;
        youtube.players.lock().await.insert("https://www.youtube.com/s/player/test/base.js".into(), Arc::new(r#"N=function(a){if(a=="bad")return "enhanced_except_error";return a.split("").reverse().join("")};"#.into()));
        for input in [
            "https://rr1.googlevideo.com/videoplayback?n=abc&sig=stable",
            "https://manifest.googlevideo.com/api/manifest/dash/n/abc/sig/a%2Fb",
        ] {
            let url = youtube
                .decipher_url(Url::parse(input).unwrap(), None, html)
                .await
                .unwrap();
            assert_eq!(url_n(&url).unwrap().unwrap().0, "cba");
            if input.contains("a%2Fb") {
                assert!(url.as_str().ends_with("sig/a%2Fb"));
            }
        }
        for input in [
            "https://rr1.googlevideo.com/videoplayback?n=a&n=b",
            "https://manifest.googlevideo.com/api/manifest/dash/n/a?n=b",
            "https://manifest.googlevideo.com/api/manifest/dash/n/",
        ] {
            assert!(
                youtube
                    .decipher_url(Url::parse(input).unwrap(), None, html)
                    .await
                    .is_err()
            );
        }
    }
    #[tokio::test]
    async fn cipher_resolution_transforms_nested_n_and_encodes_signature_once() {
        let youtube = Youtube::new().unwrap();
        let source: Arc<String> = Arc::new(r#"var H={r:function(a){a.reverse()}};S=function(a){a=a.split("");H.r(a);return a.join("")};x.sig||S(x);N=function(a){if(a=="bad")return "enhanced_except_error";return a.split("").reverse().join("")};"#.into());
        let cipher = url::form_urlencoded::Serializer::new(String::new())
            .append_pair(
                "url",
                "https://rr1.googlevideo.com/videoplayback?n=abc&keep=a%2Bb%3D&sig=stale",
            )
            .append_pair("s", "=cba")
            .append_pair("sp", "sig")
            .finish();
        let raw = json!({"itag":18,"mimeType":"video/mp4; codecs=\"avc1, mp4a\"","signatureCipher":cipher});
        let stream = youtube
            .resolve_player_format("FZ8BxMU3BYc", &raw, source.clone())
            .await
            .unwrap();
        let url = Url::parse(&stream.url).unwrap();
        let pairs: HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(pairs.get("n").unwrap(), "cba");
        assert_eq!(pairs.get("sig").unwrap(), "abc=");
        assert_eq!(pairs.get("keep").unwrap(), "a+b=");
        assert!(url.as_str().contains("sig=abc%3D"));
        assert!(!url.as_str().contains("%253D"));
        let mut encrypted = raw.clone();
        encrypted["drmFamilies"] = json!(["WIDEVINE"]);
        assert!(
            youtube
                .resolve_player_format("FZ8BxMU3BYc", &encrypted, source.clone())
                .await
                .is_err()
        );
        let disallowed =
            json!({"itag":18,"mimeType":"video/mp4","url":"https://example.invalid/media"});
        assert!(
            youtube
                .resolve_player_format("FZ8BxMU3BYc", &disallowed, source)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn provider_transport_metadata_fails_closed_before_network() {
        let raw = json!({"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","url":"https://rr1.googlevideo.com/videoplayback?itag=140","lastModified":"123","contentLength":"99","initRange":{"start":"0","end":"3"},"indexRange":{"start":"4","end":"47"}});
        let mut response = json!({"streamingData":{"serverAbrStreamingUrl":"https://rr1.googlevideo.com/videoplayback","adaptiveFormats":[raw]},"playerConfig":{"mediaCommonConfig":{"mediaUstreamerRequestConfig":{"videoPlaybackUstreamerConfig":"YQ"}}}});
        assert_eq!(Youtube::sabr_formats(&response).unwrap()[0].id, "sabr-140");
        let youtube = Youtube::new().unwrap();
        assert_eq!(
            youtube
                .dash_representations(&response, "", "FZ8BxMU3BYc")
                .await
                .unwrap()[0]
                .format
                .id,
            "dash-140"
        );
        let mut drc = response["streamingData"]["adaptiveFormats"][0].clone();
        drc["isDrc"] = json!(true);
        drc["lastModified"] = json!("456");
        response["streamingData"]["adaptiveFormats"]
            .as_array_mut()
            .unwrap()
            .insert(0, drc);
        let variants = Youtube::sabr_variants(&response).unwrap();
        assert_eq!(variants.len(), 1);
        assert_eq!(sabr_format(variants[0].0).unwrap().last_modified, 123);
        response["streamingData"]["adaptiveFormats"]
            .as_array_mut()
            .unwrap()
            .remove(0);
        let duplicate = response["streamingData"]["adaptiveFormats"][0].clone();
        response["streamingData"]["adaptiveFormats"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(Youtube::sabr_formats(&response).is_err());
        assert!(
            youtube
                .dash_representations(&response, "", "FZ8BxMU3BYc")
                .await
                .is_err()
        );
        response["streamingData"]["adaptiveFormats"]
            .as_array_mut()
            .unwrap()
            .truncate(1);
        response["streamingData"]["adaptiveFormats"][0]["drmFamilies"] = json!(["WIDEVINE"]);
        assert!(Youtube::sabr_formats(&response).is_err());
        assert!(
            youtube
                .dash_representations(&response, "", "FZ8BxMU3BYc")
                .await
                .is_err()
        );
        response["streamingData"]["adaptiveFormats"][0]
            .as_object_mut()
            .unwrap()
            .remove("drmFamilies");
        response["streamingData"]["adaptiveFormats"][0]["lastModified"] = json!("invalid");
        assert!(Youtube::sabr_formats(&response).is_err());
        response["streamingData"]["adaptiveFormats"][0]["lastModified"] = json!("123");
        response["streamingData"]["adaptiveFormats"][0]["mimeType"] =
            json!("video/mp4; codecs=\"avc1.4D401E, mp4a.40.2\"");
        assert!(Youtube::sabr_formats(&response).is_err());
    }
    #[test]
    fn replacing_signature_drops_duplicate_query_keys() {
        let mut url =
            Url::parse("https://rr1.googlevideo.com/video?sig=old&itag=140&sig=older").unwrap();
        replace_query(&mut url, "sig", "new");
        assert_eq!(url.query_pairs().filter(|(k, _)| k == "sig").count(), 1);
        assert!(url.query_pairs().any(|(k, v)| k == "itag" && v == "140"));
        assert!(url.query_pairs().any(|(k, v)| k == "sig" && v == "new"));
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    async fn server(replies: Vec<&'static str>) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!(
            "http://{}/metadata?secret=not-logged",
            listener.local_addr().unwrap()
        );
        let count = Arc::new(AtomicUsize::new(0));
        let captured = count.clone();
        tokio::spawn(async move {
            for reply in replies {
                let Ok(Ok((mut socket, _))) =
                    tokio::time::timeout(Duration::from_secs(3), listener.accept()).await
                else {
                    break;
                };
                let mut input = Vec::new();
                while !input.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    if socket.read_exact(&mut byte).await.is_err() {
                        break;
                    }
                    input.push(byte[0]);
                }
                captured.fetch_add(1, Ordering::SeqCst);
                socket.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        (address, count)
    }
    #[tokio::test]
    async fn transient_metadata_failures_retry_but_access_denials_do_not() {
        let youtube = Youtube::new().unwrap();
        let failure =
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (url, count) = server(vec![
            failure,
            "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
        ])
        .await;
        let (_, body) = youtube
            .request_body(youtube.client.get(&url))
            .await
            .unwrap();
        assert_eq!(body, "ok");
        assert_eq!(count.load(Ordering::SeqCst), 3);
        let (url, count) = server(vec![
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ])
        .await;
        let error = youtube
            .request_body(youtube.client.get(&url))
            .await
            .err()
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(error.message.contains("403"));
        assert!(!error.message.contains("secret"));
        let (url, count) = server(vec![failure, failure, failure]).await;
        assert!(
            youtube
                .request_body(youtube.client.get(&url))
                .await
                .is_err()
        );
        assert_eq!(count.load(Ordering::SeqCst), 3);
    }
    #[tokio::test]
    async fn index_fetch_requires_exact_range_and_retries_truncation() {
        let youtube = Youtube::new().unwrap();
        let (url,count) = server(vec![
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 3-5/6\r\nContent-Length: 3\r\nConnection: close\r\n\r\na",
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 3-5/6\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc",
        ]).await;
        assert_eq!(
            youtube
                .index_response(youtube.client.get(url), (3, 5), Some(6))
                .await
                .unwrap(),
            b"abc"
        );
        assert_eq!(count.load(Ordering::SeqCst), 2);
        for reply in [
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 410 Gone\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ] {
            let (url, count) = server(vec![reply]).await;
            let error = youtube
                .index_response(youtube.client.get(url), (3, 5), Some(6))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::Unavailable);
            assert_eq!(count.load(Ordering::SeqCst), 1);
        }
        for reply in [
            "HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc",
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-2/6\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc",
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 3-5/7\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc",
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 3-5/6\r\nContent-Encoding: gzip\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc",
        ] {
            let (url, count) = server(vec![reply]).await;
            assert!(
                youtube
                    .index_response(youtube.client.get(url), (3, 5), Some(6))
                    .await
                    .is_err()
            );
            assert_eq!(count.load(Ordering::SeqCst), 1);
        }
    }
    #[tokio::test]
    async fn truncated_metadata_body_retries_as_a_whole() {
        let youtube = Youtube::new().unwrap();
        let (url, count) = server(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nshort",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
        ])
        .await;
        assert_eq!(
            youtube
                .request_body(youtube.client.get(&url))
                .await
                .unwrap()
                .1,
            "ok"
        );
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}
