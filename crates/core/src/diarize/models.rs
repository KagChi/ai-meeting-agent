use serde::Deserialize;

/// Whisper transcript segment shape used by the diarize merge step.
#[derive(Debug, Clone, Deserialize)]
pub struct WhisperSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Whisper transcript envelope (only the fields the diarizer needs).
#[derive(Deserialize)]
pub struct WhisperTranscript {
    #[serde(default)]
    pub segments: Vec<WhisperSegment>,
}
