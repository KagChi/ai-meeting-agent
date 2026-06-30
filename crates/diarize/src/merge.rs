use crate::models::{CleanedSegment, SpeakerSegment, WhisperSegment};

fn overlap(a0: f64, a1: f64, b0: f32, b1: f32) -> f64 {
    (a1.min(b1 as f64) - a0.max(b0 as f64)).max(0.0)
}

pub fn merge(
    transcript: Vec<WhisperSegment>,
    speakers: Vec<SpeakerSegment>,
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
                .map(|s| (overlap(t.start, t.end, s.start, s.end), s.speaker))
                .filter(|(o, _)| *o > 0.0)
                .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
                .map(|(overlap_dur, spk)| {
                    log::debug!(
                        "[merge] segment {}: assigned speaker {} (overlap {:.2}s)",
                        idx,
                        spk,
                        overlap_dur
                    );
                    spk
                });

            if speaker.is_some() {
                assigned_count += 1;
            } else {
                unassigned_count += 1;
                log::debug!(
                    "[merge] segment {}: no overlapping speaker, assigned -1",
                    idx
                );
            }

            CleanedSegment {
                start: t.start,
                end: t.end,
                speaker: speaker.unwrap_or(-1),
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

    fn ss(start: f32, end: f32, speaker: i32) -> SpeakerSegment {
        SpeakerSegment {
            start,
            end,
            speaker,
        }
    }

    #[test]
    fn assigns_max_overlap_speaker() {
        let transcript = vec![ws(0.0, 5.0, "hello")];
        let speakers = vec![
            ss(0.0, 1.0, 0), // overlap 1.0
            ss(1.0, 4.0, 1), // overlap 3.0  <- max
            ss(4.0, 5.0, 2), // overlap 1.0
        ];
        let out = merge(transcript, speakers);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].speaker, 1);
        assert_eq!(out[0].text, "hello");
    }

    #[test]
    fn no_overlap_yields_unknown_sentinel() {
        let transcript = vec![ws(0.0, 1.0, "hi")];
        let speakers = vec![ss(2.0, 3.0, 0)];
        let out = merge(transcript, speakers);
        assert_eq!(out[0].speaker, -1);
    }

    #[test]
    fn preserves_segment_order_and_text() {
        let transcript = vec![ws(0.0, 1.0, "a"), ws(1.0, 2.0, "b")];
        let speakers = vec![ss(0.0, 2.0, 0)];
        let out = merge(transcript, speakers);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, "a");
        assert_eq!(out[1].text, "b");
        assert_eq!(out[0].speaker, 0);
        assert_eq!(out[1].speaker, 0);
    }
}
