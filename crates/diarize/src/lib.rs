pub mod audio;
pub mod config;
pub mod error;
pub mod merge;
pub mod models;
pub mod validate;

pub use config::DiarizeConfig;
pub use error::{DiarizeError, Result};
pub use merge::merge;
pub use models::{
    CleanedSegment, DiarizeResponse, SpeakerSegment, WhisperSegment, WhisperTranscript,
};
pub use validate::validate_whisper_segments;

use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig,
};

pub struct SpeakerDiarizer {
    inner: OfflineSpeakerDiarization,
}

impl SpeakerDiarizer {
    pub fn new(cfg: &DiarizeConfig) -> Result<Self> {
        let config = OfflineSpeakerDiarizationConfig {
            segmentation: OfflineSpeakerSegmentationModelConfig {
                pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: Some(cfg.segmentation_model.to_string_lossy().into_owned()),
                },
                ..Default::default()
            },
            embedding: SpeakerEmbeddingExtractorConfig {
                model: Some(cfg.embedding_model.to_string_lossy().into_owned()),
                ..Default::default()
            },
            clustering: FastClusteringConfig {
                num_clusters: cfg.num_clusters,
                threshold: cfg.clustering_threshold,
            },
            ..Default::default()
        };

        let inner = OfflineSpeakerDiarization::create(&config)
            .ok_or_else(|| DiarizeError::ModelLoadError("create() returned None".into()))?;

        Ok(Self { inner })
    }

    pub fn process(&self, samples: &[f32]) -> Result<(i32, Vec<SpeakerSegment>)> {
        log::debug!(
            "[diarizer] processing {} samples ({:.2}s of audio)",
            samples.len(),
            samples.len() as f64 / self.sample_rate() as f64
        );

        let process_start = std::time::Instant::now();
        let result = self
            .inner
            .process(samples)
            .ok_or_else(|| DiarizeError::DiarizationFailed("process() returned None".into()))?;
        let process_time = process_start.elapsed();

        let num_speakers = result.num_speakers();
        let segments = result
            .sort_by_start_time()
            .into_iter()
            .map(|s| SpeakerSegment {
                start: s.start,
                end: s.end,
                speaker: s.speaker,
            })
            .collect::<Vec<_>>();

        log::debug!(
            "[diarizer] sherpa-onnx processing complete: {} speakers detected, {} segments, took {:.2}s",
            num_speakers,
            segments.len(),
            process_time.as_secs_f64()
        );

        Ok((num_speakers, segments))
    }

    pub fn sample_rate(&self) -> i32 {
        self.inner.sample_rate()
    }
}
