use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tuneka_core::*;
pub struct FfmpegProcessor {
    slots: tokio::sync::Semaphore,
}
impl Default for FfmpegProcessor {
    fn default() -> Self {
        Self {
            slots: tokio::sync::Semaphore::new(2),
        }
    }
}
#[async_trait]
impl MediaProcessor for FfmpegProcessor {
    async fn process(
        &self,
        r: ProcessRequest,
        control: Control,
    ) -> Result<(Artifact, Vec<String>)> {
        let _permit = self
            .slots
            .acquire()
            .await
            .map_err(|_| Error::new(ErrorCode::Internal, "media pool closed"))?;
        control.check()?;
        let cancellation = Arc::new(AtomicBool::new(false));
        let flag = cancellation.clone();
        let kind = match r.format {
            OutputFormat::Mp3 => tuneka_media::OutputKind::Mp3,
            OutputFormat::M4a => tuneka_media::OutputKind::M4a,
            OutputFormat::Flac => tuneka_media::OutputKind::Flac,
            OutputFormat::Wav => tuneka_media::OutputKind::Wav,
            OutputFormat::Mp4 => tuneka_media::OutputKind::Mp4,
            OutputFormat::Mkv => tuneka_media::OutputKind::Mkv,
            OutputFormat::Webm => tuneka_media::OutputKind::Webm,
        };
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "title".into(),
            r.chapter
                .as_ref()
                .map(|c| c.title.clone())
                .unwrap_or(r.title),
        );
        let missing_cover = r.cover.is_none()
            && matches!(
                r.format,
                OutputFormat::Mp3 | OutputFormat::M4a | OutputFormat::Flac
            );
        let request = tuneka_media::MediaRequest {
            input: r.input,
            additional_audio: r.additional_audio,
            output: r.output,
            kind,
            start_seconds: r.chapter.as_ref().map(|c| c.start),
            end_seconds: r.chapter.and_then(|c| c.end),
            metadata,
            cover: r.cover,
            cancellation,
        };
        let receipt = r.publication_receipt;
        let mut worker = tokio::task::spawn_blocking(move || {
            tuneka_media::process_with_receipt(&request, &receipt)
        });
        let result = loop {
            tokio::select! {r=&mut worker=>break r.map_err(|_|Error::new(ErrorCode::Internal,"media worker failed"))?,_ = tokio::time::sleep(std::time::Duration::from_millis(25))=>{if control.check().is_err(){flag.store(true,Ordering::SeqCst);}}}
        };
        // Publication is a commit point: preserve its artifact even if pause/cancel arrived
        // just after commit; scheduler records it before acknowledging the control request.
        match result {
            Ok(r) => {
                let mut warnings = r.warnings;
                if missing_cover {
                    warnings.push("Cover unavailable; output has no cover".into());
                }
                Ok((
                    Artifact {
                        path: r.output,
                        size: r.bytes,
                    },
                    warnings,
                ))
            }
            Err(e) => {
                control.check()?;
                Err(Error::new(ErrorCode::Media, e.to_string()))
            }
        }
    }
}
/// Keep native media diagnostics from writing over a terminal UI.
pub fn quiet_native_media_logs() {
    tuneka_media::quiet_native_logs();
}

pub fn engine() -> Result<Arc<Engine>> {
    Engine::new(
        Arc::new(tuneka_youtube::Youtube::new()?),
        Arc::new(tuneka_download::HttpDownloader::new()?),
        Arc::new(FfmpegProcessor::default()),
        settings::data_dir(),
    )
}
pub fn exit_code(s: &JobSnapshot) -> u8 {
    match s.state {
        JobState::Completed => 0,
        JobState::PartiallyCompleted => 2,
        JobState::Cancelled => 130,
        JobState::Paused => 75,
        _ => 1,
    }
}
