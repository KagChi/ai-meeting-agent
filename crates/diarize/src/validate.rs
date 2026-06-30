use crate::models::WhisperSegment;

pub fn validate_whisper_segments(raw: Vec<WhisperSegment>) -> Vec<WhisperSegment> {
    let input_count = raw.len();
    let mut filtered_empty = 0;
    let mut filtered_invalid_time = 0;

    let valid: Vec<WhisperSegment> = raw
        .into_iter()
        .filter(|s| {
            if s.text.trim().is_empty() {
                filtered_empty += 1;
                return false;
            }
            if !s.start.is_finite() || !s.end.is_finite() || s.start >= s.end {
                filtered_invalid_time += 1;
                return false;
            }
            true
        })
        .collect();

    let total_filtered = filtered_empty + filtered_invalid_time;
    if total_filtered > 0 {
        log::debug!(
            "[validate] filtered {} segments: {} empty text, {} invalid timestamps; {} valid remain",
            total_filtered,
            filtered_empty,
            filtered_invalid_time,
            valid.len()
        );
    } else {
        log::debug!("[validate] all {} segments valid", input_count);
    }

    valid
}
