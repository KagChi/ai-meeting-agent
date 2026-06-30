use crate::models::WhisperSegment;

pub fn validate_whisper_segments(raw: Vec<WhisperSegment>) -> Vec<WhisperSegment> {
    raw.into_iter()
        .filter(|s| {
            !s.text.trim().is_empty() && s.start.is_finite() && s.end.is_finite() && s.start < s.end
        })
        .collect()
}
