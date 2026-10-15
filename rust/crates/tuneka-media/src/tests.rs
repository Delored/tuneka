use super::*;

fn tone(path: &Path, seconds: f64) {
    ffmpeg::init().unwrap();
    let codec = ffmpeg::encoder::find(codec::Id::PCM_S16LE).unwrap();
    let mut output = format::output_as(path, "wav").unwrap();
    let mut encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .audio()
        .unwrap();
    encoder.set_rate(48_000);
    encoder.set_channel_layout(ffmpeg::ChannelLayout::MONO);
    encoder.set_format(ffmpeg::format::Sample::I16(
        ffmpeg::format::sample::Type::Packed,
    ));
    encoder.set_time_base((1, 48_000));
    let mut encoder = encoder.open_as(codec).unwrap();
    {
        let mut stream = output.add_stream(codec).unwrap();
        stream.set_parameters(&encoder);
        stream.set_time_base((1, 48_000));
    }
    output.write_header().unwrap();
    let time_base = output.stream(0).unwrap().time_base();
    let total = (seconds * 48_000.0) as usize;
    let mut position = 0;
    while position < total {
        let count = (total - position).min(1024);
        let mut audio = frame::Audio::new(encoder.format(), count, ffmpeg::ChannelLayout::MONO);
        audio.set_rate(48_000);
        audio.set_pts(Some(position as i64));
        for (index, sample) in audio.plane_mut::<i16>(0).iter_mut().enumerate() {
            *sample = ((std::f64::consts::TAU * 440.0 * (position + index) as f64 / 48_000.0).sin()
                * 8_000.0) as i16;
        }
        encoder.send_frame(&audio).unwrap();
        drain_encoder(&mut encoder, &mut output, time_base);
        position += count;
    }
    encoder.send_eof().unwrap();
    drain_encoder(&mut encoder, &mut output, time_base);
    output.write_trailer().unwrap();
}
fn drain_encoder(
    encoder: &mut codec::encoder::Audio,
    output: &mut format::context::Output,
    time_base: ffmpeg::Rational,
) {
    let mut packet = Packet::empty();
    while encoder.receive_packet(&mut packet).is_ok() {
        packet.set_stream(0);
        packet.rescale_ts(encoder.time_base(), time_base);
        packet.write_interleaved(output).unwrap();
    }
}
fn sample_count(path: &Path) -> usize {
    let mut source = input(path).unwrap();
    let stream = source.streams().best(media::Type::Audio).unwrap();
    let index = stream.index();
    let mut decoder = codec::context::Context::from_parameters(stream.parameters())
        .unwrap()
        .decoder()
        .audio()
        .unwrap();
    let mut count = 0;
    for (stream, packet) in source.packets() {
        if stream.index() == index {
            decoder.send_packet(&packet).unwrap();
            let mut frame = frame::Audio::empty();
            while decoder.receive_frame(&mut frame).is_ok() {
                count += frame.samples();
            }
        }
    }
    decoder.send_eof().unwrap();
    let mut frame = frame::Audio::empty();
    while decoder.receive_frame(&mut frame).is_ok() {
        count += frame.samples();
    }
    count
}

#[test]
fn real_audio_formats_trim_tags_and_decode() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("tone.wav");
    tone(&source, 1.0);
    assert_eq!(sample_count(&source), 48_000);
    for (kind, extension, codec) in [
        (OutputKind::Mp3, "mp3", "mp3"),
        (OutputKind::M4a, "m4a", "aac"),
        (OutputKind::Flac, "flac", "flac"),
        (OutputKind::Wav, "wav", "pcm_s16le"),
    ] {
        let output = dir.path().join(format!("trim.{extension}"));
        let mut request = MediaRequest::new(&source, &output, kind);
        request.start_seconds = Some(0.35);
        request.end_seconds = Some(0.8);
        request
            .metadata
            .insert("title".into(), "Тестовий трек".into());
        let result = process(&request).unwrap();
        assert!(result.bytes > 100);
        assert_eq!(result.probe.streams[0].codec, codec);
        assert_eq!(
            result.probe.metadata.get("title").map(String::as_str),
            Some("Тестовий трек")
        );
        let samples = sample_count(&output);
        // Lossy codecs pad to codec frame boundaries; lossless trim must be exact.
        if matches!(kind, OutputKind::Wav | OutputKind::Flac) {
            assert_eq!(samples, 21_600);
        } else {
            assert!(
                (21_000..24_000).contains(&samples),
                "{kind:?} decoded {samples} samples"
            );
        }
        assert!((0.44..0.53).contains(&result.probe.duration_seconds.unwrap()));
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 5);
}

#[test]
fn cancellation_conflict_invalid_trim_and_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("tone.wav");
    tone(&source, 0.1);
    let target = dir.path().join("output.flac");
    let mut request = MediaRequest::new(&source, &target, OutputKind::Flac);
    request.cancellation.store(true, Ordering::Relaxed);
    assert!(matches!(process(&request), Err(MediaError::Cancelled)));
    request.cancellation.store(false, Ordering::Relaxed);
    request.start_seconds = Some(3.0);
    assert!(process(&request).is_err());
    assert!(!target.exists());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    request.start_seconds = None;
    request.cover = Some(dir.path().join("unavailable.jpg"));
    let result = process(&request).unwrap();
    assert_eq!(result.warnings.len(), 1);
    let original = fs::read(&target).unwrap();
    assert!(process(&request).is_err());
    assert_eq!(original, fs::read(&target).unwrap());
    request.output = dir.path().join("bad.flac");
    request.end_seconds = Some(f64::NAN);
    assert!(matches!(process(&request), Err(MediaError::Invalid(_))));
}

fn video(path: &Path, id: codec::Id, container: &str) {
    video_with_timestamps(path, id, container, 0, 0);
}
fn video_with_timestamps(
    path: &Path,
    id: codec::Id,
    container: &str,
    origin_frames: i64,
    b_frames: usize,
) {
    ffmpeg::init().unwrap();
    let codec = ffmpeg::encoder::find(id).expect("test video encoder must be installed");
    let mut output = format::output_as(path, container).unwrap();
    let mut encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()
        .unwrap();
    encoder.set_width(64);
    encoder.set_height(48);
    encoder.set_format(ffmpeg::format::Pixel::YUV420P);
    encoder.set_time_base((1, 25));
    encoder.set_frame_rate(Some((25, 1)));
    encoder.set_max_b_frames(b_frames);
    encoder.set_gop(50);
    if output
        .format()
        .flags()
        .contains(format::flag::Flags::GLOBAL_HEADER)
    {
        encoder.set_flags(codec::flag::Flags::GLOBAL_HEADER);
    }
    let mut options = ffmpeg::Dictionary::new();
    if id == codec::Id::H264 && b_frames > 0 {
        options.set("x264-params", "bframes=2:b-adapt=0:scenecut=0:keyint=50");
    }
    let mut encoder = encoder.open_as_with(codec, options).unwrap();
    {
        let mut stream = output.add_stream(codec).unwrap();
        stream.set_parameters(&encoder);
        stream.set_time_base((1, 25));
    }
    output.write_header().unwrap();
    let time_base = output.stream(0).unwrap().time_base();
    for position in 0..25 {
        let mut image = frame::Video::new(ffmpeg::format::Pixel::YUV420P, 64, 48);
        image.data_mut(0).fill(32 + position as u8);
        image.data_mut(1).fill(128);
        image.data_mut(2).fill(128);
        image.set_pts(Some(position + origin_frames));
        encoder.send_frame(&image).unwrap();
        let mut packet = Packet::empty();
        while encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(0);
            packet.set_duration(1);
            packet.rescale_ts(encoder.time_base(), time_base);
            packet.write_interleaved(&mut output).unwrap();
        }
    }
    encoder.send_eof().unwrap();
    let mut packet = Packet::empty();
    while encoder.receive_packet(&mut packet).is_ok() {
        packet.set_stream(0);
        packet.set_duration(1);
        packet.rescale_ts(encoder.time_base(), time_base);
        packet.write_interleaved(&mut output).unwrap();
    }
    output.write_trailer().unwrap();
}

#[test]
fn real_video_mux_remux_and_container_policy() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("video.mp4");
    video(&source, codec::Id::H264, "mp4");
    let tone_file = dir.path().join("tone.wav");
    tone(&tone_file, 1.0);
    let audio = dir.path().join("audio.m4a");
    process(&MediaRequest::new(&tone_file, &audio, OutputKind::M4a)).unwrap();
    let combined = dir.path().join("combined.mp4");
    let mut request = MediaRequest::new(&source, &combined, OutputKind::Mp4);
    request.additional_audio = Some(audio);
    request
        .metadata
        .insert("title".into(), "Mux fixture".into());
    let result = process(&request).unwrap();
    assert_eq!(result.probe.streams.len(), 2);
    assert_eq!(video_frames(&combined), 25);
    assert_eq!(result.probe.streams[0].codec, "h264");
    assert_eq!(result.probe.streams[1].codec, "aac");
    assert!((0.98..1.1).contains(&result.probe.duration_seconds.unwrap()));
    // Check both streams' decoded durations and that packet timestamps stay monotonic.
    assert!((47_500..50_000).contains(&sample_count(&combined)));
    let mut context = input(&combined).unwrap();
    let mut last = [i64::MIN; 2];
    for (stream, packet) in context.packets() {
        let timestamp = packet.dts().unwrap();
        assert!(timestamp >= last[stream.index()]);
        last[stream.index()] = timestamp;
    }
    let mkv = dir.path().join("combined.mkv");
    let remuxed = process(&MediaRequest::new(&combined, &mkv, OutputKind::Mkv)).unwrap();
    assert_eq!(remuxed.probe.streams.len(), 2);
    let webm = dir.path().join("unsupported.webm");
    assert!(matches!(
        process(&MediaRequest::new(&combined, &webm, OutputKind::Webm)),
        Err(MediaError::Unsupported(_))
    ));
    assert!(!webm.exists());
    request.output = dir.path().join("trim.mp4");
    request.start_seconds = Some(0.1);
    assert!(matches!(process(&request), Err(MediaError::Unsupported(_))));
}

#[test]
fn active_cancellation_cleans_temporary_output() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("long.wav");
    tone(&source, 30.0);
    let request = MediaRequest::new(&source, dir.path().join("cancel.mp3"), OutputKind::Mp3);
    let cancellation = request.cancellation.clone();
    let watch = dir.path().to_owned();
    let handle = std::thread::spawn(move || process(&request));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if fs::read_dir(&watch).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".part")
        }) {
            cancellation.store(true, Ordering::Relaxed);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(matches!(handle.join().unwrap(), Err(MediaError::Cancelled)));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

fn cover_png(path: &Path) {
    ffmpeg::init().unwrap();
    let codec = ffmpeg::encoder::find(codec::Id::PNG).unwrap();
    let mut encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()
        .unwrap();
    encoder.set_width(16);
    encoder.set_height(16);
    encoder.set_format(ffmpeg::format::Pixel::RGB24);
    encoder.set_time_base((1, 1));
    let mut encoder = encoder.open_as(codec).unwrap();
    let mut image = frame::Video::new(ffmpeg::format::Pixel::RGB24, 16, 16);
    image.data_mut(0).fill(120);
    encoder.send_frame(&image).unwrap();
    let mut packet = Packet::empty();
    encoder.receive_packet(&mut packet).unwrap();
    fs::write(path, packet.data().unwrap()).unwrap();
}

#[test]
fn real_embedded_cover_and_explicit_unsupported_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("tone.wav");
    tone(&source, 0.2);
    let cover = dir.path().join("cover.png");
    cover_png(&cover);
    for (kind, extension) in [
        (OutputKind::Mp3, "mp3"),
        (OutputKind::M4a, "m4a"),
        (OutputKind::Flac, "flac"),
        (OutputKind::Wav, "wav"),
    ] {
        let mut request =
            MediaRequest::new(&source, dir.path().join(format!("cover.{extension}")), kind);
        request.cover = Some(cover.clone());
        let result = process(&request).unwrap();
        if kind == OutputKind::Wav {
            assert_eq!(result.warnings.len(), 1);
            assert!(result.probe.streams.iter().all(|s| !s.is_cover));
        } else {
            assert!(result.warnings.is_empty());
            assert!(
                result
                    .probe
                    .streams
                    .iter()
                    .any(|s| s.codec == "png" && s.is_cover),
                "{kind:?}"
            );
        }
        assert!(sample_count(&result.output) >= 9_600);
    }
}

fn video_frames(path: &Path) -> usize {
    let mut source = input(path).unwrap();
    let stream = source.streams().best(media::Type::Video).unwrap();
    let index = stream.index();
    let mut decoder = codec::context::Context::from_parameters(stream.parameters())
        .unwrap()
        .decoder()
        .video()
        .unwrap();
    let mut count = 0;
    for (stream, packet) in source.packets() {
        if stream.index() == index {
            decoder.send_packet(&packet).unwrap();
            let mut frame = frame::Video::empty();
            while decoder.receive_frame(&mut frame).is_ok() {
                count += 1;
            }
        }
    }
    decoder.send_eof().unwrap();
    let mut frame = frame::Video::empty();
    while decoder.receive_frame(&mut frame).is_ok() {
        count += 1;
    }
    count
}

#[test]
fn webm_real_video_remux_and_decode() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.webm");
    video(&source, codec::Id::VP8, "webm");
    let result = process(&MediaRequest::new(
        &source,
        dir.path().join("copy.webm"),
        OutputKind::Webm,
    ))
    .unwrap();
    assert_eq!(result.probe.streams[0].codec, "vp8");
    assert_eq!(video_frames(&result.output), 25);
    assert!(
        (0.99..1.01).contains(&result.probe.duration_seconds.unwrap()),
        "source {:?}, output {:?}",
        probe(&source).unwrap().duration_seconds,
        result.probe.duration_seconds
    );
}

#[test]
fn competing_outputs_never_overwrite_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("tone.wav");
    tone(&source, 1.0);
    let request = MediaRequest::new(&source, dir.path().join("shared.flac"), OutputKind::Flac);
    let other = request.clone();
    let first = std::thread::spawn(move || process(&other));
    let second = process(&request);
    let first = first.join().unwrap();
    assert_ne!(first.is_ok(), second.is_ok());
    assert_eq!(sample_count(&request.output), 48_000);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

fn millisecond_aac(path: &Path) {
    ffmpeg::init().unwrap();
    let codec = ffmpeg::encoder::find(codec::Id::AAC).unwrap();
    let mut output = format::output_as(path, "matroska").unwrap();
    let mut encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .audio()
        .unwrap();
    encoder.set_rate(48_000);
    encoder.set_channel_layout(ffmpeg::ChannelLayout::MONO);
    encoder.set_format(ffmpeg::format::Sample::F32(
        ffmpeg::format::sample::Type::Planar,
    ));
    encoder.set_time_base((1, 48_000));
    encoder.set_flags(codec::flag::Flags::GLOBAL_HEADER);
    let mut encoder = encoder.open_as(codec).unwrap();
    {
        let mut stream = output.add_stream(codec).unwrap();
        stream.set_parameters(&encoder);
        stream.set_time_base((1, 1000));
    }
    output.write_header().unwrap();
    let output_time_base = output.stream(0).unwrap().time_base();
    assert_eq!(output_time_base, ffmpeg::Rational(1, 1000));
    for index in 0..120 {
        let mut audio = frame::Audio::new(encoder.format(), 1024, ffmpeg::ChannelLayout::MONO);
        audio.set_rate(48_000);
        audio.set_pts(Some(index * 1024));
        for (offset, sample) in audio.plane_mut::<f32>(0).iter_mut().enumerate() {
            *sample = (std::f64::consts::TAU * 440.0 * (index as usize * 1024 + offset) as f64
                / 48_000.0)
                .sin() as f32
                * 0.2;
        }
        encoder.send_frame(&audio).unwrap();
        drain_encoder(&mut encoder, &mut output, output_time_base);
    }
    encoder.send_eof().unwrap();
    drain_encoder(&mut encoder, &mut output, output_time_base);
    output.write_trailer().unwrap();
}

#[test]
fn millisecond_input_timebase_preserves_sample_duration_and_packet_timestamps() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("milliseconds.mka");
    millisecond_aac(&source);
    let context = input(&source).unwrap();
    let stream = context.streams().best(media::Type::Audio).unwrap();
    assert_eq!(stream.time_base(), ffmpeg::Rational(1, 1000));
    let decoder = codec::context::Context::from_parameters(stream.parameters())
        .unwrap()
        .decoder()
        .audio()
        .unwrap();
    assert_eq!(
        decoder.format(),
        ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Planar)
    );
    let source_samples = sample_count(&source);
    let expected_seconds = source_samples as f64 / 48_000.0;
    for (kind, extension) in [
        (OutputKind::M4a, "m4a"),
        (OutputKind::Mp3, "mp3"),
        (OutputKind::Flac, "flac"),
        (OutputKind::Wav, "wav"),
    ] {
        let output = dir.path().join(format!("converted.{extension}"));
        let result = process(&MediaRequest::new(&source, &output, kind)).unwrap();
        let decoded_seconds = sample_count(&output) as f64 / 48_000.0;
        let declared_seconds = result.probe.duration_seconds.unwrap();
        assert!(
            (declared_seconds - expected_seconds).abs() < 0.05,
            "{kind:?}: container {declared_seconds}s, source samples {expected_seconds}s"
        );
        assert!(
            (decoded_seconds - expected_seconds).abs() < 0.05,
            "{kind:?}: decoded {decoded_seconds}s, source {expected_seconds}s"
        );
        let mut context = input(&output).unwrap();
        let mut first = None;
        let mut last_end = 0.0f64;
        let mut last_dts = i64::MIN;
        for (stream, packet) in context.packets() {
            if stream.parameters().medium() != media::Type::Audio {
                continue;
            }
            let dts = packet.dts().unwrap();
            assert!(dts >= last_dts);
            last_dts = dts;
            let time_base = f64::from(stream.time_base());
            first.get_or_insert(packet.pts().unwrap() as f64 * time_base);
            last_end = last_end.max((packet.pts().unwrap() + packet.duration()) as f64 * time_base);
        }
        let timeline_seconds = last_end - first.unwrap();
        assert!(
            (timeline_seconds - expected_seconds).abs() < 0.06,
            "{kind:?}: packet timeline {timeline_seconds}s, sample duration {expected_seconds}s"
        );
        let trimmed_output = dir.path().join(format!("trimmed.{extension}"));
        let mut request = MediaRequest::new(&source, &trimmed_output, kind);
        request.start_seconds = Some(0.35);
        request.end_seconds = Some(1.15);
        let trimmed = process(&request).unwrap();
        let trimmed_samples = sample_count(&trimmed_output);
        let tolerance = if matches!(kind, OutputKind::Flac | OutputKind::Wav) {
            48
        } else {
            2048
        };
        assert!(
            trimmed_samples.abs_diff(38_400) <= tolerance,
            "{kind:?}: trimmed {trimmed_samples} samples"
        );
        assert!((trimmed.probe.duration_seconds.unwrap() - 0.8).abs() < 0.05);
    }
}

fn offset_audio(path: &Path, id: codec::Id, container: &str, origin_samples: i64) {
    ffmpeg::init().unwrap();
    let codec = ffmpeg::encoder::find(id).unwrap();
    let mut output = format::output_as(path, container).unwrap();
    let mut encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .audio()
        .unwrap();
    encoder.set_rate(48_000);
    encoder.set_channel_layout(ffmpeg::ChannelLayout::MONO);
    let sample_format = if id == codec::Id::AAC {
        ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Planar)
    } else {
        ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed)
    };
    encoder.set_format(sample_format);
    encoder.set_time_base((1, 48_000));
    if output
        .format()
        .flags()
        .contains(format::flag::Flags::GLOBAL_HEADER)
    {
        encoder.set_flags(codec::flag::Flags::GLOBAL_HEADER);
    }
    let mut encoder = encoder.open_as(codec).unwrap();
    {
        let mut stream = output.add_stream(codec).unwrap();
        stream.set_parameters(&encoder);
        stream.set_time_base((1, 48_000));
    }
    output.write_header().unwrap();
    let time_base = output.stream(0).unwrap().time_base();
    let frame_size = encoder.frame_size() as usize;
    let mut position = 0;
    while position < 48_000 {
        let count = (48_000 - position).min(frame_size);
        let mut frame = frame::Audio::new(sample_format, count, ffmpeg::ChannelLayout::MONO);
        frame.set_rate(48_000);
        frame.set_pts(Some(origin_samples + position as i64));
        for (index, sample) in frame.plane_mut::<f32>(0).iter_mut().enumerate() {
            *sample = (std::f64::consts::TAU * 440.0 * (position + index) as f64 / 48_000.0).sin()
                as f32
                * 0.2;
        }
        encoder.send_frame(&frame).unwrap();
        drain_encoder(&mut encoder, &mut output, time_base);
        position += count;
    }
    encoder.send_eof().unwrap();
    drain_encoder(&mut encoder, &mut output, time_base);
    output.write_trailer().unwrap();
}

#[derive(Debug)]
struct DecodedTimeline {
    first: f64,
    end: f64,
    frames: usize,
    b_frames: usize,
    samples: usize,
}
fn decoded_timeline(path: &Path, medium: media::Type) -> DecodedTimeline {
    let mut input = input(path).unwrap();
    let stream = input.streams().best(medium).unwrap();
    let index = stream.index();
    let time_base = f64::from(stream.time_base());
    let mut timeline = DecodedTimeline {
        first: f64::INFINITY,
        end: f64::NEG_INFINITY,
        frames: 0,
        b_frames: 0,
        samples: 0,
    };
    if medium == media::Type::Audio {
        let mut decoder = codec::context::Context::from_parameters(stream.parameters())
            .unwrap()
            .decoder()
            .audio()
            .unwrap();
        let mut receive = |decoder: &mut codec::decoder::Audio| {
            let mut frame = frame::Audio::empty();
            while decoder.receive_frame(&mut frame).is_ok() {
                let start = frame.timestamp().unwrap() as f64 * time_base;
                timeline.first = timeline.first.min(start);
                timeline.end = timeline
                    .end
                    .max(start + frame.samples() as f64 / f64::from(frame.rate()));
                timeline.frames += 1;
                timeline.samples += frame.samples();
            }
        };
        for (stream, packet) in input.packets() {
            if stream.index() == index {
                decoder.send_packet(&packet).unwrap();
                receive(&mut decoder);
            }
        }
        decoder.send_eof().unwrap();
        receive(&mut decoder);
    } else {
        let mut decoder = codec::context::Context::from_parameters(stream.parameters())
            .unwrap()
            .decoder()
            .video()
            .unwrap();
        let mut receive = |decoder: &mut codec::decoder::Video| {
            let mut frame = frame::Video::empty();
            while decoder.receive_frame(&mut frame).is_ok() {
                let start = frame.timestamp().unwrap() as f64 * time_base;
                timeline.first = timeline.first.min(start);
                timeline.end = timeline.end.max(start + 0.04);
                timeline.frames += 1;
                if frame.kind() == ffmpeg::picture::Type::B {
                    timeline.b_frames += 1;
                }
            }
        };
        for (stream, packet) in input.packets() {
            if stream.index() == index {
                decoder.send_packet(&packet).unwrap();
                receive(&mut decoder);
            }
        }
        decoder.send_eof().unwrap();
        receive(&mut decoder);
    }
    timeline
}
fn assert_sync_preserved(
    path: &Path,
    video: &DecodedTimeline,
    audio: &DecodedTimeline,
    tolerance: f64,
) {
    let actual_video = decoded_timeline(path, media::Type::Video);
    let actual_audio = decoded_timeline(path, media::Type::Audio);
    assert_eq!(actual_video.frames, video.frames);
    assert_eq!(actual_video.b_frames, video.b_frames);
    assert_eq!(
        actual_audio.samples,
        audio.samples,
        "audio sample count changed in {}",
        path.display()
    );
    let original_offset = audio.first - video.first;
    let actual_offset = actual_audio.first - actual_video.first;
    assert!(
        (actual_offset - original_offset).abs() <= tolerance,
        "{}: relative start changed {original_offset}s -> {actual_offset}s; {actual_video:?}; {actual_audio:?}",
        path.display()
    );
    assert!(
        ((actual_audio.end - actual_video.end) - (audio.end - video.end)).abs() <= tolerance,
        "relative end changed in {}",
        path.display()
    );
    assert!((actual_video.end - actual_video.first - (video.end - video.first)).abs() <= tolerance);
    let mut input = input(path).unwrap();
    let mut dts = vec![None; input.nb_streams() as usize];
    for (stream, packet) in input.packets() {
        if let Some(timestamp) = packet.dts() {
            if let Some(previous) = dts[stream.index()] {
                assert!(timestamp >= previous, "nonmonotonic DTS");
            }
            dts[stream.index()] = Some(timestamp);
        }
    }
}

#[test]
fn b_frames_and_nonzero_origins_preserve_relative_av_sync_mp4_mkv() {
    let dir = tempfile::tempdir().unwrap();
    for origin_frames in [0, 125] {
        let root = dir.path().join(format!("origin-{origin_frames}"));
        fs::create_dir(&root).unwrap();
        let video = root.join("delayed-video.mp4");
        video_with_timestamps(&video, codec::Id::H264, "mp4", origin_frames, 2);
        let audio = root.join("delayed-audio.m4a");
        let origin_seconds = origin_frames as f64 / 25.0;
        offset_audio(&audio, codec::Id::AAC, "mp4", origin_frames * 1920 + 14_400);
        let video_timeline = decoded_timeline(&video, media::Type::Video);
        let audio_timeline = decoded_timeline(&audio, media::Type::Audio);
        assert!((video_timeline.first - origin_seconds).abs() < 0.001);
        assert!(
            video_timeline.b_frames > 0,
            "fixture must decode actual B-frames"
        );
        assert!((audio_timeline.first - origin_seconds - 0.3).abs() < 0.025);
        let mut context = input(&video).unwrap();
        assert!(
            context
                .packets()
                .any(|(_, packet)| packet.pts() != packet.dts()),
            "fixture must actually reorder frames"
        );
        if origin_frames == 0 {
            let mut context = input(&video).unwrap();
            assert!(
                context
                    .packets()
                    .any(|(_, packet)| packet.dts().is_some_and(|ts| ts < 0)),
                "zero-origin B-frame fixture must include negative DTS"
            );
        }
        for (kind, extension) in [(OutputKind::Mp4, "mp4"), (OutputKind::Mkv, "mkv")] {
            let combined = root.join(format!("combined.{extension}"));
            let mut request = MediaRequest::new(&video, &combined, kind);
            request.additional_audio = Some(audio.clone());
            process(&request).unwrap();
            assert_sync_preserved(&combined, &video_timeline, &audio_timeline, 0.002);
            for (target_kind, target_extension) in
                [(OutputKind::Mp4, "mp4"), (OutputKind::Mkv, "mkv")]
            {
                let remuxed = root.join(format!("remuxed-from-{extension}.{target_extension}"));
                process(&MediaRequest::new(&combined, &remuxed, target_kind)).unwrap();
                assert_sync_preserved(&remuxed, &video_timeline, &audio_timeline, 0.002);
            }
        }
    }
}

#[test]
fn nonzero_origin_vp8_opus_mux_remux_preserves_webm_sync() {
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("video.webm");
    video_with_timestamps(&video, codec::Id::VP8, "webm", 125, 0);
    let audio = dir.path().join("audio.webm");
    offset_audio(&audio, codec::Id::OPUS, "webm", 254_400);
    let video_timeline = decoded_timeline(&video, media::Type::Video);
    let audio_timeline = decoded_timeline(&audio, media::Type::Audio);
    assert!((video_timeline.first - 5.0).abs() < 0.001);
    assert!(
        (audio_timeline.first - 5.3).abs() < 0.01,
        "{audio_timeline:?}"
    );
    let combined = dir.path().join("combined.webm");
    let mut request = MediaRequest::new(&video, &combined, OutputKind::Webm);
    request.additional_audio = Some(audio);
    process(&request).unwrap();
    assert_sync_preserved(&combined, &video_timeline, &audio_timeline, 0.002);
    let remuxed = dir.path().join("remuxed.webm");
    process(&MediaRequest::new(&combined, &remuxed, OutputKind::Webm)).unwrap();
    assert_sync_preserved(&remuxed, &video_timeline, &audio_timeline, 0.002);
}

#[test]
fn local_hls_transport_stream_is_demuxed_without_manifest_protocols() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("segments.ts");
    video_with_timestamps(&source, codec::Id::H264, "mpegts", 125, 2);
    let output = dir.path().join("video.mp4");
    process(&MediaRequest::new(&source, &output, OutputKind::Mp4)).unwrap();
    assert_eq!(video_frames(&output), 25);
    let manifest = dir.path().join("forbidden.m3u8");
    std::fs::write(
        &manifest,
        "#EXTM3U\n#EXTINF:1,\nhttps://localhost/secret.ts\n#EXT-X-ENDLIST\n",
    )
    .unwrap();
    assert!(
        probe(&manifest).is_err(),
        "FFmpeg must not fetch manifests itself"
    );
}
