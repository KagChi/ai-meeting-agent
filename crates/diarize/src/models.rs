use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct SpeakerSegment {
    pub start: f32,
    pub end: f32,
    pub speaker: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WhisperSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct CleanedSegment {
    pub start: f64,
    pub end: f64,
    pub speaker: i32,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct DiarizeResponse {
    pub num_speakers: i32,
    pub segments: Vec<CleanedSegment>,
}

#[derive(Deserialize)]
pub struct WhisperTranscript {
    #[serde(default)]
    pub segments: Vec<WhisperSegment>,
}
