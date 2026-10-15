use std::path::Path;
use tuneka_cli::FfmpegProcessor;
use tuneka_core::{files::recover_publication, *};

// Self-contained uncompressed source; all product processing is through FFmpeg APIs.
fn wav(path: &Path) {
    let samples = 4_800u32;
    let bytes = samples * 2;
    let mut data = Vec::new();
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(36 + bytes).to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&48_000u32.to_le_bytes());
    data.extend_from_slice(&96_000u32.to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&bytes.to_le_bytes());
    for i in 0..samples {
        let sample = ((i as f64 * std::f64::consts::TAU * 440.0 / 48_000.0).sin() * 5_000.0) as i16;
        data.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, data).unwrap();
}

#[tokio::test]
async fn real_ffmpeg_composition_receipt_recovers_and_rejects_digest_corruption() {
    let d = tempfile::tempdir().unwrap();
    let input = d.path().join("tone.wav");
    wav(&input);
    let output = d.path().join("tone.flac");
    let receipt = d.path().join("publication.json");
    let processor = FfmpegProcessor::default();
    let (artifact, warnings) = processor
        .process(
            ProcessRequest {
                input,
                additional_audio: None,
                output: output.clone(),
                format: OutputFormat::Flac,
                chapter: None,
                title: "Recovery fixture".into(),
                cover: None,
                publication_receipt: receipt.clone(),
            },
            Control::default(),
        )
        .await
        .unwrap();
    assert_eq!(warnings.len(), 1); // Missing cover is explicit.
    // The caller has not persisted the returned artifact yet: recover solely from journal.
    let recovered = recover_publication(&receipt, &output).unwrap().unwrap();
    assert_eq!(recovered.path, artifact.path);
    assert_eq!(recovered.size, artifact.size);
    let probe = tuneka_media::probe(&output).unwrap();
    assert_eq!(probe.streams[0].codec, "flac");
    assert!((probe.duration_seconds.unwrap() - 0.1).abs() < 0.001);
    let original = std::fs::read(&output).unwrap();
    let mut changed = original.clone();
    let last = changed.len() - 1;
    changed[last] ^= 1;
    std::fs::write(&output, &changed).unwrap();
    assert_eq!(
        recover_publication(&receipt, &output).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(std::fs::read(&output).unwrap(), changed);
    std::fs::write(&output, &original).unwrap();
    let mut journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
    journal["sha256"] = serde_json::Value::String("0".repeat(64));
    std::fs::write(&receipt, serde_json::to_vec(&journal).unwrap()).unwrap();
    assert_eq!(
        recover_publication(&receipt, &output).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(std::fs::read(&output).unwrap(), original);
}
