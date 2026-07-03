use crate::diarize::models::WhisperSegment;

/// A transcript segment with an assigned speaker label.
/// `speaker = None` means no overlapping speaker segment was found.
#[derive(Debug, Clone)]
pub struct CleanedSegment {
    pub start: f64,
    pub end: f64,
    pub speaker: Option<String>,
    pub text: String,
}

fn overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// Assign each Whisper transcript segment the speaker label of the
/// speakrs segment with the maximum time overlap. Returns one
/// `CleanedSegment` per input Whisper segment, preserving order.
pub fn merge(
    transcript: Vec<WhisperSegment>,
    speakers: &[speakrs::Segment],
) -> Vec<CleanedSegment> {
    log::debug!(
        "[merge] merging {} transcript segments with {} speaker segments",
        transcript.len(),
        speakers.len()
    );

    let mut assigned_count = 0;
    let mut unassigned_count = 0;

    let result: Vec<CleanedSegment> = transcript
        .iter()
        .enumerate()
        .map(|(idx, t)| {
            let speaker = speakers
                .iter()
                .map(|s| (overlap(t.start, t.end, s.start, s.end), &s.speaker))
                .filter(|(o, _)| *o > 0.0)
                .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
                .map(|(overlap_dur, spk)| {
                    log::debug!(
                        "[merge] segment {}: assigned speaker {} (overlap {:.2}s)",
                        idx,
                        spk,
                        overlap_dur
                    );
                    spk.clone()
                });

            if speaker.is_some() {
                assigned_count += 1;
            } else {
                unassigned_count += 1;
                log::debug!("[merge] segment {}: no overlapping speaker", idx);
            }

            CleanedSegment {
                start: t.start,
                end: t.end,
                speaker,
                text: t.text.clone(),
            }
        })
        .collect();

    log::debug!(
        "[merge] complete: {} assigned, {} unassigned",
        assigned_count,
        unassigned_count
    );

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(start: f64, end: f64, text: &str) -> WhisperSegment {
        WhisperSegment {
            start,
            end,
            text: text.into(),
        }
    }

    fn seg(start: f64, end: f64, speaker: &str) -> speakrs::Segment {
        speakrs::Segment::new(start, end, speaker)
    }

    #[test]
    fn assigns_max_overlap_speaker() {
        let transcript = vec![ws(0.0, 5.0, "hello")];
        let speakers = vec![
            seg(0.0, 1.0, "SPEAKER_00"), // overlap 1.0
            seg(1.0, 4.0, "SPEAKER_01"), // overlap 3.0  <- max
            seg(4.0, 5.0, "SPEAKER_02"), // overlap 1.0
        ];
        let out = merge(transcript, &speakers);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].speaker.as_deref(), Some("SPEAKER_01"));
        assert_eq!(out[0].text, "hello");
    }

    #[test]
    fn no_overlap_yields_none() {
        let transcript = vec![ws(0.0, 1.0, "hi")];
        let speakers = vec![seg(2.0, 3.0, "SPEAKER_00")];
        let out = merge(transcript, &speakers);
        assert_eq!(out[0].speaker, None);
    }

    #[test]
    fn preserves_segment_order_and_text() {
        let transcript = vec![ws(0.0, 1.0, "a"), ws(1.0, 2.0, "b")];
        let speakers = vec![seg(0.0, 2.0, "SPEAKER_00")];
        let out = merge(transcript, &speakers);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, "a");
        assert_eq!(out[1].text, "b");
        assert_eq!(out[0].speaker.as_deref(), Some("SPEAKER_00"));
        assert_eq!(out[1].speaker.as_deref(), Some("SPEAKER_00"));
    }
}
