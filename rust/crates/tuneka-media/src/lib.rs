//! Local media processing through linked FFmpeg libraries. Blocking: call from a worker.
//! The decoder/filter/encoder flow follows ffmpeg-next's transcode-audio example
//! (https://github.com/zmwangx/rust-ffmpeg, WTFPL v2).

use ffmpeg::{Packet, Rescale, codec, filter, format, frame, media};
use ffmpeg_next as ffmpeg;
use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

/// Silence process-wide native FFmpeg diagnostics before starting TUI workers.
/// Processing failures still return typed errors to the caller.
pub fn quiet_native_logs() {
    ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Quiet);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    Mp3,
    M4a,
    Flac,
    Wav,
    Mp4,
    Mkv,
    Webm,
}
impl OutputKind {
    fn container(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::M4a | Self::Mp4 => "mp4",
            Self::Flac => "flac",
            Self::Wav => "wav",
            Self::Mkv => "matroska",
            Self::Webm => "webm",
        }
    }
    fn is_audio(self) -> bool {
        matches!(self, Self::Mp3 | Self::M4a | Self::Flac | Self::Wav)
    }
}

#[derive(Debug, Clone)]
pub struct MediaRequest {
    pub input: PathBuf,
    /// A separate audio stream to mux with the first input's video.
    pub additional_audio: Option<PathBuf>,
    pub output: PathBuf,
    pub kind: OutputKind,
    pub start_seconds: Option<f64>,
    pub end_seconds: Option<f64>,
    pub metadata: BTreeMap<String, String>,
    pub cover: Option<PathBuf>,
    pub cancellation: Arc<AtomicBool>,
}
impl MediaRequest {
    pub fn new(input: impl Into<PathBuf>, output: impl Into<PathBuf>, kind: OutputKind) -> Self {
        Self {
            input: input.into(),
            additional_audio: None,
            output: output.into(),
            kind,
            start_seconds: None,
            end_seconds: None,
            metadata: BTreeMap::new(),
            cover: None,
            cancellation: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MediaResult {
    pub output: PathBuf,
    pub bytes: u64,
    pub warnings: Vec<String>,
    pub probe: MediaProbe,
}
#[derive(Debug, Clone)]
pub struct MediaProbe {
    pub duration_seconds: Option<f64>,
    pub streams: Vec<StreamProbe>,
    pub metadata: BTreeMap<String, String>,
}
#[derive(Debug, Clone)]
pub struct StreamProbe {
    pub kind: String,
    pub codec: String,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub is_cover: bool,
}

#[derive(Debug)]
pub enum MediaError {
    Io(std::io::Error),
    Ffmpeg(ffmpeg::Error),
    Invalid(String),
    Unsupported(String),
    Cancelled,
}
impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "media I/O: {e}"),
            Self::Ffmpeg(e) => write!(f, "FFmpeg: {e}"),
            Self::Invalid(e) => write!(f, "invalid media request: {e}"),
            Self::Unsupported(e) => write!(f, "unsupported media operation: {e}"),
            Self::Cancelled => write!(f, "media processing cancelled"),
        }
    }
}
impl std::error::Error for MediaError {}
impl From<std::io::Error> for MediaError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<ffmpeg::Error> for MediaError {
    fn from(e: ffmpeg::Error) -> Self {
        Self::Ffmpeg(e)
    }
}
type Result<T> = std::result::Result<T, MediaError>;

fn check_path(path: &Path) -> Result<()> {
    if path.to_str().is_none_or(|s| s.contains('\0')) {
        return Err(MediaError::Invalid(
            "FFmpeg paths must be UTF-8 without NUL".into(),
        ));
    }
    Ok(())
}
fn check_cancel(request: &MediaRequest) -> Result<()> {
    if request.cancellation.load(Ordering::Relaxed) {
        Err(MediaError::Cancelled)
    } else {
        Ok(())
    }
}
fn input(path: &Path) -> Result<format::context::Input> {
    check_path(path)?;
    if !path.is_file() {
        return Err(MediaError::Invalid(
            "input must be an existing local file".into(),
        ));
    }
    // Restrict demuxer nested resources: downloaded media cannot fetch remote URLs,
    // crypto resources, or HLS/DASH referenced local files.
    let mut options = ffmpeg::Dictionary::new();
    options.set("protocol_whitelist", "file");
    options.set(
        "format_whitelist",
        "mov,mp3,flac,wav,matroska,webm,ogg,aac,mpegts",
    );
    Ok(format::input_with_dictionary(path, options)?)
}

pub fn probe(path: impl AsRef<Path>) -> Result<MediaProbe> {
    ffmpeg::init()?;
    let context = input(path.as_ref())?;
    let mut streams = Vec::new();
    for stream in context.streams() {
        let parameters = stream.parameters();
        let kind = parameters.medium();
        let codec = parameters.id().name().to_string();
        let (sample_rate, channels) = if kind == media::Type::Audio {
            let decoder = codec::context::Context::from_parameters(parameters)?
                .decoder()
                .audio()?;
            (Some(decoder.rate()), Some(decoder.channels()))
        } else {
            (None, None)
        };
        let is_cover = stream
            .disposition()
            .contains(format::stream::Disposition::ATTACHED_PIC);
        streams.push(StreamProbe {
            is_cover,
            kind: format!("{kind:?}").to_lowercase(),
            codec,
            sample_rate,
            channels,
        });
    }
    Ok(MediaProbe {
        duration_seconds: (context.duration() > 0)
            .then(|| context.duration() as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE)),
        streams,
        metadata: context
            .metadata()
            .iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect(),
    })
}

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn temporary(output: &Path) -> Result<Temporary> {
    check_path(output)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    for _ in 0..100 {
        let path = parent.join(format!(
            ".tuneka-media-{}-{}.part",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => return Ok(Temporary(path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(MediaError::Invalid(
        "could not reserve temporary output".into(),
    ))
}

/// Produces a complete output atomically without replacing existing files.
/// Audio trimming is sample accurate; video trimming requires re-encoding and is rejected.
pub fn process(request: &MediaRequest) -> Result<MediaResult> {
    process_inner(request, None)
}
/// Journal a verified output before publication so a crashed coordinator can adopt it.
pub fn process_with_receipt(request: &MediaRequest, receipt: &Path) -> Result<MediaResult> {
    process_inner(request, Some(receipt))
}
fn process_inner(request: &MediaRequest, receipt: Option<&Path>) -> Result<MediaResult> {
    ffmpeg::init()?;
    check_cancel(request)?;
    check_path(&request.output)?;
    if request.output.exists() {
        return Err(MediaError::Invalid("output already exists".into()));
    }
    let start = request.start_seconds.unwrap_or(0.0);
    if !start.is_finite()
        || start < 0.0
        || request
            .end_seconds
            .is_some_and(|v| !v.is_finite() || v <= start)
    {
        return Err(MediaError::Invalid(
            "trim bounds must be finite, nonnegative and increasing".into(),
        ));
    }
    if request
        .metadata
        .iter()
        .any(|(k, v)| k.contains('\0') || v.contains('\0'))
    {
        return Err(MediaError::Invalid("metadata cannot contain NUL".into()));
    }
    let temp = temporary(&request.output)?;
    let mut warnings = Vec::new();
    let mut cover = match request.cover.as_ref() {
        Some(path)
            if matches!(
                request.kind,
                OutputKind::Mp3 | OutputKind::M4a | OutputKind::Flac | OutputKind::Mp4
            ) =>
        {
            match read_cover(path) {
                Ok(cover) => Some(cover),
                Err(_) => {
                    warnings.push("Cover could not be read as a local JPEG or PNG; output contains no requested cover".into());
                    None
                }
            }
        }
        Some(_) => {
            warnings.push("Requested container does not support this cover attachment; output contains no requested cover".into());
            None
        }
        None => None,
    };
    if request.kind.is_audio() {
        if request.additional_audio.is_some() {
            return Err(MediaError::Unsupported(
                "additional audio is only accepted for video muxing".into(),
            ));
        }
        transcode_audio(request, &temp.0, cover.as_mut())?;
    } else {
        remux(request, &temp.0, cover.as_mut())?;
    }
    check_cancel(request)?;
    let probe = probe(&temp.0)?;
    if probe.streams.is_empty() {
        return Err(MediaError::Invalid("output has no media streams".into()));
    }
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&temp.0)?
        .sync_all()?;
    let bytes = fs::metadata(&temp.0)?.len();
    if let Some(receipt) = receipt {
        use sha2::{Digest, Sha256};
        use std::io::{Read, Write};
        let mut source = fs::File::open(&temp.0)?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            check_cancel(request)?;
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        let json = serde_json::json!({"path":request.output,"size":bytes,"sha256":format!("{:x}",hash.finalize())});
        let receipt_temp = temporary(receipt)?;
        let mut file = fs::File::create(&receipt_temp.0)?;
        file.write_all(json.to_string().as_bytes())?;
        file.sync_all()?;
        fs::rename(&receipt_temp.0, receipt)?;
    }
    check_cancel(request)?;
    // Hard-link publication fails on a competing target; rename would overwrite on Unix.
    fs::hard_link(&temp.0, &request.output)?;
    Ok(MediaResult {
        output: request.output.clone(),
        bytes,
        warnings,
        probe,
    })
}

struct Cover {
    parameters: codec::Parameters,
    packet: Packet,
}
fn read_cover(path: &Path) -> Result<Cover> {
    check_path(path)?;
    if !path.is_file() || fs::metadata(path)?.len() > 20 * 1024 * 1024 {
        return Err(MediaError::Invalid(
            "cover must be a local image below 20 MiB".into(),
        ));
    }
    let mut options = ffmpeg::Dictionary::new();
    options.set("protocol_whitelist", "file");
    options.set("format_whitelist", "image2,jpeg_pipe,png_pipe");
    let mut source = format::input_with_dictionary(path, options)?;
    let stream = source
        .streams()
        .best(media::Type::Video)
        .ok_or_else(|| MediaError::Invalid("cover has no image stream".into()))?;
    let parameters = stream.parameters().clone();
    if !matches!(parameters.id(), codec::Id::MJPEG | codec::Id::PNG) {
        return Err(MediaError::Unsupported("cover must be JPEG or PNG".into()));
    }
    let mut packet = Packet::empty();
    packet.read(&mut source)?;
    packet.set_pts(Some(0));
    packet.set_dts(Some(0));
    packet.set_duration(0);
    Ok(Cover { parameters, packet })
}
fn add_cover_stream(output: &mut format::context::Output, cover: &mut Cover) -> Result<()> {
    let mut stream = output.add_stream(ffmpeg::encoder::find(codec::Id::None))?;
    stream.set_parameters(cover.parameters.clone());
    stream.set_time_base((1, 90_000));
    let mut tags = ffmpeg::Dictionary::new();
    tags.set("title", "Cover");
    tags.set("comment", "Cover (front)");
    stream.set_metadata(tags);
    // SAFETY: stream and codec parameters are owned by the uniquely borrowed
    // output. ffmpeg-next exposes no disposition setter; assign AVStream's flag.
    unsafe {
        (*stream.as_mut_ptr()).disposition = ffmpeg::ffi::AV_DISPOSITION_ATTACHED_PIC;
        (*stream.parameters().as_mut_ptr()).codec_tag = 0;
    }
    cover.packet.set_stream(stream.index());
    Ok(())
}

fn set_metadata(
    output: &mut format::context::Output,
    source: &format::context::Input,
    request: &MediaRequest,
) {
    let mut tags = source.metadata().to_owned();
    for (key, value) in &request.metadata {
        tags.set(key, value);
    }
    output.set_metadata(tags);
}
fn drained(error: ffmpeg::Error) -> Result<()> {
    match error {
        ffmpeg::Error::Eof => Ok(()),
        ffmpeg::Error::Other { errno } if errno == ffmpeg::error::EAGAIN => Ok(()),
        e => Err(e.into()),
    }
}

// ffmpeg-next rejects empty payloads even when they carry side data. FLAC's
// final NEW_EXTRADATA packet has exactly this shape and updates STREAMINFO.
fn write_packet(packet: &mut Packet, output: &mut format::context::Output) -> Result<()> {
    if packet.size() != 0 {
        return Ok(packet.write_interleaved(output)?);
    }
    use ffmpeg::codec::packet::Mut;
    // SAFETY: both objects are live and uniquely borrowed; av_interleaved_write_frame
    // takes ownership of the packet reference and resets it, as the safe wrapper does.
    let result = unsafe {
        ffmpeg::ffi::av_interleaved_write_frame(output.as_mut_ptr(), packet.as_mut_ptr())
    };
    if result < 0 {
        return Err(ffmpeg::Error::from(result).into());
    }
    Ok(())
}

struct AudioPipeline {
    decoder: codec::decoder::Audio,
    encoder: codec::encoder::Audio,
    filter: filter::Graph,
    output_time_base: ffmpeg::Rational,
    samples: usize,
}
impl AudioPipeline {
    fn packets(&mut self, output: &mut format::context::Output) -> Result<()> {
        let mut packet = Packet::empty();
        loop {
            match self.encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    packet.set_stream(0);
                    packet.rescale_ts(self.encoder.time_base(), self.output_time_base);
                    write_packet(&mut packet, output)?;
                }
                Err(e) => return drained(e),
            }
        }
    }
    fn filtered(&mut self, output: &mut format::context::Output) -> Result<()> {
        // The sink keeps the input time base when no rate/sample-format conversion
        // inserts a resampler. In particular, FLTP 48 kHz WebM -> AAC can keep
        // 1/1000 timestamps, whereas the encoder requires 1/48000 timestamps.
        let filter_time_base = self
            .filter
            .get("out")
            .expect("configured sink")
            .sink()
            .time_base();
        let encoder_time_base = self.encoder.time_base();
        let mut audio = frame::Audio::empty();
        loop {
            match self
                .filter
                .get("out")
                .expect("configured sink")
                .sink()
                .frame(&mut audio)
            {
                Ok(()) => {
                    let timestamp = audio
                        .pts()
                        .map(|pts| pts.rescale(filter_time_base, encoder_time_base));
                    audio.set_pts(timestamp);
                    self.samples += audio.samples();
                    self.encoder.send_frame(&audio)?;
                    self.packets(output)?;
                }
                Err(e) => return drained(e),
            }
        }
    }
    fn decoded(
        &mut self,
        output: &mut format::context::Output,
        request: &MediaRequest,
    ) -> Result<()> {
        let mut audio = frame::Audio::empty();
        loop {
            check_cancel(request)?;
            match self.decoder.receive_frame(&mut audio) {
                Ok(()) => {
                    let timestamp = audio.timestamp();
                    audio.set_pts(timestamp);
                    match self
                        .filter
                        .get("in")
                        .expect("configured source")
                        .source()
                        .add(&audio)
                    {
                        Ok(()) | Err(ffmpeg::Error::Eof) => {}
                        Err(e) => return Err(e.into()),
                    };
                    self.filtered(output)?;
                }
                Err(e) => return drained(e),
            }
        }
    }
}
fn transcode_audio(
    request: &MediaRequest,
    path: &Path,
    mut cover: Option<&mut Cover>,
) -> Result<()> {
    let mut source = input(&request.input)?;
    let stream = source
        .streams()
        .best(media::Type::Audio)
        .ok_or_else(|| MediaError::Invalid("input has no audio".into()))?;
    let stream_index = stream.index();
    let time_base = stream.time_base();
    let mut decoder = codec::context::Context::from_parameters(stream.parameters())?
        .decoder()
        .audio()?;
    if decoder.channel_layout().is_empty() {
        decoder.set_channel_layout(ffmpeg::ChannelLayout::default(i32::from(
            decoder.channels(),
        )));
    }
    let mut output = format::output_as(path, request.kind.container())?;
    let codec_id = match request.kind {
        OutputKind::Mp3 => codec::Id::MP3,
        OutputKind::M4a => codec::Id::AAC,
        OutputKind::Flac => codec::Id::FLAC,
        OutputKind::Wav => codec::Id::PCM_S16LE,
        _ => unreachable!(),
    };
    let codec = ffmpeg::encoder::find(codec_id)
        .ok_or_else(|| {
            MediaError::Unsupported(format!("encoder {} is not installed", codec_id.name()))
        })?
        .audio()?;
    let mut encoder = codec::context::Context::new_with_codec(*codec)
        .encoder()
        .audio()?;
    if output
        .format()
        .flags()
        .contains(format::flag::Flags::GLOBAL_HEADER)
    {
        encoder.set_flags(codec::flag::Flags::GLOBAL_HEADER);
    }
    let layout = codec
        .channel_layouts()
        .map(|v| v.best(i32::from(decoder.channels())))
        .unwrap_or(decoder.channel_layout());
    let rate = codec
        .rates()
        .map(|rates| {
            rates
                .min_by_key(|r| (i64::from(*r) - i64::from(decoder.rate())).abs())
                .unwrap_or(decoder.rate() as i32)
        })
        .unwrap_or(decoder.rate() as i32);
    encoder.set_rate(rate);
    encoder.set_channel_layout(layout);
    encoder.set_format(
        codec
            .formats()
            .and_then(|mut formats| formats.next())
            .ok_or_else(|| {
                MediaError::Unsupported("encoder has no supported sample format".into())
            })?,
    );
    encoder.set_bit_rate(192_000);
    encoder.set_time_base((1, rate));
    let encoder = encoder.open_as(codec)?;
    {
        let mut stream = output.add_stream(codec)?;
        stream.set_parameters(&encoder);
        stream.set_time_base((1, rate));
    }
    let mut graph = filter::Graph::new();
    let args = format!(
        "time_base={}:sample_rate={}:sample_fmt={}:channel_layout=0x{:x}",
        time_base,
        decoder.rate(),
        decoder.format().name(),
        decoder.channel_layout().bits()
    );
    graph.add(
        &filter::find("abuffer")
            .ok_or_else(|| MediaError::Unsupported("abuffer filter unavailable".into()))?,
        "in",
        &args,
    )?;
    graph.add(
        &filter::find("abuffersink")
            .ok_or_else(|| MediaError::Unsupported("abuffersink filter unavailable".into()))?,
        "out",
        "",
    )?;
    {
        let mut sink = graph.get("out").expect("configured sink");
        sink.set_sample_format(encoder.format());
        sink.set_channel_layout(encoder.channel_layout());
        sink.set_sample_rate(encoder.rate());
    }
    let bounds = match request.end_seconds {
        Some(end) => format!("start={}:end={end}", request.start_seconds.unwrap_or(0.0)),
        None => format!("start={}", request.start_seconds.unwrap_or(0.0)),
    };
    graph.output("in", 0)?.input("out", 0)?.parse(&format!(
        "asetpts=PTS-STARTPTS,atrim={bounds},asetpts=PTS-STARTPTS"
    ))?;
    graph.validate()?;
    if !codec
        .capabilities()
        .contains(codec::capabilities::Capabilities::VARIABLE_FRAME_SIZE)
    {
        graph
            .get("out")
            .expect("configured sink")
            .sink()
            .set_frame_size(encoder.frame_size());
    }
    set_metadata(&mut output, &source, request);
    if let Some(cover) = cover.as_mut() {
        add_cover_stream(&mut output, cover)?;
    }
    output.write_header()?;
    if let Some(cover) = cover.as_mut() {
        cover.packet.write_interleaved(&mut output)?;
    }
    let mut pipeline = AudioPipeline {
        decoder,
        encoder,
        filter: graph,
        output_time_base: output.stream(0).expect("output stream").time_base(),
        samples: 0,
    };
    loop {
        check_cancel(request)?;
        let mut packet = Packet::empty();
        match packet.read(&mut source) {
            Ok(()) => {}
            Err(ffmpeg::Error::Eof) => break,
            Err(e) => return Err(e.into()),
        }
        if packet.stream() == stream_index {
            pipeline.decoder.send_packet(&packet)?;
            pipeline.decoded(&mut output, request)?;
        }
    }
    pipeline.decoder.send_eof()?;
    pipeline.decoded(&mut output, request)?;
    pipeline
        .filter
        .get("in")
        .expect("configured source")
        .source()
        .flush()?;
    pipeline.filtered(&mut output)?;
    if pipeline.samples == 0 {
        return Err(MediaError::Invalid("trim contains no audio samples".into()));
    }
    pipeline.encoder.send_eof()?;
    pipeline.packets(&mut output)?;
    output.write_trailer()?;
    Ok(())
}

fn compatible(kind: OutputKind, id: codec::Id) -> bool {
    match kind {
        OutputKind::Mp4 => matches!(
            id,
            codec::Id::H264
                | codec::Id::HEVC
                | codec::Id::AV1
                | codec::Id::AAC
                | codec::Id::MP3
                | codec::Id::ALAC
        ),
        OutputKind::Webm => matches!(
            id,
            codec::Id::VP8 | codec::Id::VP9 | codec::Id::AV1 | codec::Id::OPUS | codec::Id::VORBIS
        ),
        OutputKind::Mkv => matches!(
            id,
            codec::Id::H264
                | codec::Id::HEVC
                | codec::Id::AV1
                | codec::Id::VP8
                | codec::Id::VP9
                | codec::Id::AAC
                | codec::Id::MP3
                | codec::Id::ALAC
                | codec::Id::OPUS
                | codec::Id::VORBIS
                | codec::Id::FLAC
                | codec::Id::PCM_S16LE
        ),
        _ => false,
    }
}
struct CopyStream {
    input_index: usize,
    output_index: usize,
    input_time_base: ffmpeg::Rational,
    output_time_base: ffmpeg::Rational,
}
fn add_copy_stream(
    source: &format::context::Input,
    output: &mut format::context::Output,
    medium: media::Type,
    kind: OutputKind,
) -> Result<Option<CopyStream>> {
    let Some(stream) = source.streams().best(medium) else {
        return Ok(None);
    };
    if medium == media::Type::Video
        && stream
            .disposition()
            .contains(format::stream::Disposition::ATTACHED_PIC)
    {
        return Err(MediaError::Invalid(
            "input has only an attached picture, no video".into(),
        ));
    }
    let parameters = stream.parameters();
    if !compatible(kind, parameters.id()) {
        return Err(MediaError::Unsupported(format!(
            "codec {} cannot be safely copied to {:?}; transcoding is required",
            parameters.id().name(),
            kind
        )));
    }
    let mut target = output.add_stream(ffmpeg::encoder::find(codec::Id::None))?;
    target.set_parameters(parameters);
    // SAFETY: target owns valid mutable AVCodecParameters. Clearing the source
    // container's tag lets the destination muxer choose its native codec tag.
    unsafe {
        (*target.parameters().as_mut_ptr()).codec_tag = 0;
    }
    target.set_time_base(stream.time_base());
    target.set_rate(stream.rate());
    target.set_avg_frame_rate(stream.avg_frame_rate());
    Ok(Some(CopyStream {
        input_index: stream.index(),
        output_index: target.index(),
        input_time_base: stream.time_base(),
        output_time_base: stream.time_base(),
    }))
}
fn next_copy_packet(
    source: &mut format::context::Input,
    streams: &[CopyStream],
    request: &MediaRequest,
) -> Result<Option<Packet>> {
    loop {
        check_cancel(request)?;
        let mut packet = Packet::empty();
        match packet.read(source) {
            Ok(()) => {}
            Err(ffmpeg::Error::Eof) => return Ok(None),
            Err(e) => return Err(e.into()),
        }
        if let Some(stream) = streams.iter().find(|s| s.input_index == packet.stream()) {
            packet.rescale_ts(stream.input_time_base, stream.output_time_base);
            packet.set_stream(stream.output_index);
            packet.set_position(-1);
            return Ok(Some(packet));
        }
    }
}
fn packet_seconds(packet: &Packet, streams: &[CopyStream]) -> f64 {
    let time_base = streams
        .iter()
        .find(|s| s.output_index == packet.stream())
        .expect("mapped packet")
        .output_time_base;
    packet.dts().or(packet.pts()).unwrap_or(0) as f64 * f64::from(time_base)
}
fn remux(request: &MediaRequest, path: &Path, mut cover: Option<&mut Cover>) -> Result<()> {
    if request.start_seconds.is_some() || request.end_seconds.is_some() {
        return Err(MediaError::Unsupported(
            "sample-accurate video trimming requires re-encoding".into(),
        ));
    }
    let mut source = input(&request.input)?;
    let mut audio = request
        .additional_audio
        .as_ref()
        .map(|path| input(path))
        .transpose()?;
    let mut output = format::output_as(path, request.kind.container())?;
    let mut main_streams = Vec::new();
    main_streams.push(
        add_copy_stream(&source, &mut output, media::Type::Video, request.kind)?
            .ok_or_else(|| MediaError::Invalid("input has no video".into()))?,
    );
    let mut audio_streams = Vec::new();
    if let Some(audio) = &audio {
        audio_streams.push(
            add_copy_stream(audio, &mut output, media::Type::Audio, request.kind)?
                .ok_or_else(|| MediaError::Invalid("additional input has no audio".into()))?,
        );
    } else if let Some(stream) =
        add_copy_stream(&source, &mut output, media::Type::Audio, request.kind)?
    {
        main_streams.push(stream);
    }
    set_metadata(&mut output, &source, request);
    if let Some(cover) = cover.as_mut() {
        add_cover_stream(&mut output, cover)?;
    }
    output.write_header()?;
    if let Some(cover) = cover.as_mut() {
        cover.packet.write_interleaved(&mut output)?;
    }
    for stream in main_streams.iter_mut().chain(audio_streams.iter_mut()) {
        stream.output_time_base = output
            .stream(stream.output_index)
            .expect("mapped stream")
            .time_base();
    }
    let mut main_packet = next_copy_packet(&mut source, &main_streams, request)?;
    let mut audio_packet = match audio.as_mut() {
        Some(audio) => next_copy_packet(audio, &audio_streams, request)?,
        None => None,
    };
    let mut written = 0;
    while main_packet.is_some() || audio_packet.is_some() {
        check_cancel(request)?;
        let use_main = match (&main_packet, &audio_packet) {
            (Some(main), Some(audio)) => {
                packet_seconds(main, &main_streams) <= packet_seconds(audio, &audio_streams)
            }
            (Some(_), None) => true,
            _ => false,
        };
        if use_main {
            main_packet
                .take()
                .expect("available main packet")
                .write_interleaved(&mut output)?;
            main_packet = next_copy_packet(&mut source, &main_streams, request)?;
        } else {
            audio_packet
                .take()
                .expect("available audio packet")
                .write_interleaved(&mut output)?;
            audio_packet = next_copy_packet(
                audio.as_mut().expect("audio input"),
                &audio_streams,
                request,
            )?;
        }
        written += 1;
    }
    if written == 0 {
        return Err(MediaError::Invalid("input has no packets".into()));
    }
    output.write_trailer()?;
    Ok(())
}

#[cfg(test)]
mod tests;
