//! Voice bank: rebuild centroids from enrollment samples + identify match.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::Utc;
use uuid::Uuid;

use crate::config::DiarizeConfig;
use crate::models::{Voiceprint, VoiceprintEnrolledFrom};
use crate::storage::MeetingStorage;
use crate::transcription::TranscriptionResponse;

#[cfg(feature = "diarization")]
use crate::diarize::embed::{
    cosine_similarity, embed_audio_file, mean_pool_l2, EMBED_DIM, EMBED_MODEL_ID,
};

/// Default minimum total enrolled speech (seconds) before building a centroid.
pub const DEFAULT_ENROLL_MIN_SPEECH_S: f64 = 30.0;

/// Default cosine threshold for person match (tuned later on lab voices).
pub const DEFAULT_IDENTIFY_THRESHOLD: f32 = 0.55;

/// Result of matching a query embedding against the voice bank.
#[derive(Debug, Clone)]
pub struct IdentifyMatch {
    pub person_id: Option<String>,
    pub confidence: f32,
    /// Display name if matched, else `Guest-N` style label assigned by caller.
    pub label: Option<String>,
}

/// Rebuild the person's voiceprint centroid from all enrollment samples on disk.
///
/// Requires feature `diarization` (WeSpeaker). Returns `None` if total sample
/// duration is below `min_speech_s` (still keeps samples for later).
#[cfg(feature = "diarization")]
pub async fn rebuild_voiceprint(
    storage: &MeetingStorage,
    person_id: &str,
    cfg: &DiarizeConfig,
    min_speech_s: f64,
) -> anyhow::Result<Option<Voiceprint>> {
    let _person = storage.get_person(person_id).await?;
    let samples = storage.list_voiceprint_samples(person_id).await?;
    if samples.is_empty() {
        return Ok(None);
    }

    let total_dur: f64 = samples.iter().map(|s| s.duration_s).sum();
    if total_dur + f64::EPSILON < min_speech_s {
        log::info!(
            "[voiceprint] person {person_id}: total speech {total_dur:.1}s < min {min_speech_s:.1}s; skip centroid"
        );
        return Ok(None);
    }

    let mut embeddings = Vec::new();
    for sample in &samples {
        let abs = storage.voiceprint_sample_abs_path(sample);
        if !abs.exists() {
            log::warn!(
                "[voiceprint] sample file missing: {}; skip",
                abs.display()
            );
            continue;
        }
        match embed_audio_file(&abs, None, None, cfg) {
            Ok(vec) => embeddings.push(vec),
            Err(e) => {
                log::warn!(
                    "[voiceprint] embed failed for {}: {e:#}",
                    abs.display()
                );
            }
        }
    }

    if embeddings.is_empty() {
        anyhow::bail!("no samples could be embedded for person {person_id}");
    }

    let centroid = crate::diarize::embed::mean_pool_l2(&embeddings)?;
    if centroid.len() != EMBED_DIM as usize {
        anyhow::bail!(
            "unexpected embed dim {} (expected {EMBED_DIM})",
            centroid.len()
        );
    }

    let now = Utc::now();
    let existing = storage.get_voiceprint(person_id).await?;
    let vp = Voiceprint {
        id: existing
            .as_ref()
            .map(|v| v.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        person_id: person_id.to_string(),
        model: EMBED_MODEL_ID.to_string(),
        dim: EMBED_DIM,
        centroid,
        enrolled_from: VoiceprintEnrolledFrom::Sample,
        created_at: existing
            .as_ref()
            .map(|v| v.created_at)
            .unwrap_or(now),
        updated_at: now,
    };
    storage.upsert_voiceprint(&vp).await?;
    log::info!(
        "[voiceprint] rebuilt centroid for person {person_id} from {} samples",
        embeddings.len()
    );
    Ok(Some(vp))
}

#[cfg(not(feature = "diarization"))]
pub async fn rebuild_voiceprint(
    _storage: &MeetingStorage,
    _person_id: &str,
    _cfg: &DiarizeConfig,
    _min_speech_s: f64,
) -> anyhow::Result<Option<Voiceprint>> {
    anyhow::bail!("voiceprint rebuild requires the `diarization` feature")
}

/// Match a query embedding against all enrolled voiceprints.
#[cfg(feature = "diarization")]
pub async fn match_embedding(
    storage: &MeetingStorage,
    query: &[f32],
    threshold: f32,
) -> anyhow::Result<IdentifyMatch> {
    let bank = storage.list_voiceprints().await?;
    if bank.is_empty() {
        return Ok(IdentifyMatch {
            person_id: None,
            confidence: 0.0,
            label: None,
        });
    }

    let mut best_id: Option<String> = None;
    let mut best_score = f32::NEG_INFINITY;
    for vp in &bank {
        if vp.centroid.len() != query.len() {
            continue;
        }
        let score = cosine_similarity(query, &vp.centroid);
        if score > best_score {
            best_score = score;
            best_id = Some(vp.person_id.clone());
        }
    }

    if best_score >= threshold {
        Ok(IdentifyMatch {
            person_id: best_id,
            confidence: best_score,
            label: None, // caller fills name from Person
        })
    } else {
        Ok(IdentifyMatch {
            person_id: None,
            confidence: best_score.max(0.0),
            label: None,
        })
    }
}

#[cfg(not(feature = "diarization"))]
pub async fn match_embedding(
    _storage: &MeetingStorage,
    _query: &[f32],
    _threshold: f32,
) -> anyhow::Result<IdentifyMatch> {
    anyhow::bail!("voiceprint match requires the `diarization` feature")
}

/// One speaker-label assignment after identify.
#[derive(Debug, Clone)]
pub struct SpeakerIdentity {
    /// Original diarization label (e.g. `SPEAKER_00`).
    pub diar_label: String,
    /// Display name written to `speaker` (`Alice` or `Guest-1`).
    pub display_name: String,
    pub person_id: Option<String>,
    pub confidence: Option<f32>,
    /// Total speech duration used for embedding.
    pub speech_s: f64,
}

/// Summary of an identify run.
#[derive(Debug, Clone)]
pub struct IdentifyResult {
    pub identities: Vec<SpeakerIdentity>,
    pub matched: u32,
    pub guests: u32,
    pub skipped: u32,
}

/// Identify diarized speakers in a transcript against the voice bank.
///
/// For each unique non-empty `speaker` label:
/// 1. Collect segment time spans
/// 2. Embed those spans from `audio_path` (mean-pool)
/// 3. Cosine-match vs enrolled centroids
/// 4. ≥ threshold → person name + `person_id`; else `Guest-N`
///
/// Mutates `transcript.segments` speaker/person_id/confidence in place.
/// Does **not** write to DB — caller applies via storage or saves transcript.
#[cfg(feature = "diarization")]
pub async fn identify_transcript(
    audio_path: &Path,
    transcript: &mut TranscriptionResponse,
    storage: &MeetingStorage,
    cfg: &DiarizeConfig,
    threshold: f32,
) -> anyhow::Result<IdentifyResult> {
    let bank = storage.list_voiceprints().await?;
    let persons = storage.list_persons().await?;
    let name_by_id: HashMap<String, String> = persons
        .into_iter()
        .map(|p| (p.id, p.name))
        .collect();

    let Some(segments) = transcript.segments.as_mut() else {
        return Ok(IdentifyResult {
            identities: Vec::new(),
            matched: 0,
            guests: 0,
            skipped: 0,
        });
    };

    // Unique labels preserving first-seen order
    let mut labels: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for seg in segments.iter() {
        if let Some(sp) = seg.speaker.as_ref() {
            let t = sp.trim();
            if !t.is_empty() && seen.insert(t.to_string()) {
                labels.push(t.to_string());
            }
        }
    }

    if labels.is_empty() {
        return Ok(IdentifyResult {
            identities: Vec::new(),
            matched: 0,
            guests: 0,
            skipped: 0,
        });
    }

    let mut identities = Vec::new();
    let mut matched = 0u32;
    let mut guests = 0u32;
    let mut skipped = 0u32;
    let mut guest_n = 0u32;

    // Preload bank for matching without re-query
    let bank_empty = bank.is_empty();

    for label in &labels {
        let spans: Vec<(f64, f64)> = segments
            .iter()
            .filter(|s| s.speaker.as_deref() == Some(label.as_str()))
            .map(|s| (s.start, s.end))
            .collect();
        let speech_s: f64 = spans.iter().map(|(a, b)| (b - a).max(0.0)).sum();

        if speech_s < 0.5 {
            skipped += 1;
            log::info!(
                "[identify] skip {label}: only {speech_s:.2}s speech"
            );
            continue;
        }

        // Embed each span ≥ ~0.25s, mean-pool
        let mut embs = Vec::new();
        for (start, end) in &spans {
            if end - start < 0.25 {
                continue;
            }
            match embed_audio_file(audio_path, Some(*start), Some(*end), cfg) {
                Ok(v) => embs.push(v),
                Err(e) => {
                    log::debug!(
                        "[identify] embed span {start:.2}-{end:.2} for {label} failed: {e:#}"
                    );
                }
            }
        }

        if embs.is_empty() {
            skipped += 1;
            log::warn!("[identify] no embeddable spans for {label}");
            continue;
        }

        let query = mean_pool_l2(&embs)?;
        let (person_id, confidence, display_name) = if bank_empty {
            guest_n += 1;
            guests += 1;
            (None, None, format!("Guest-{guest_n}"))
        } else {
            let m = match_embedding(storage, &query, threshold).await?;
            if let Some(pid) = m.person_id {
                matched += 1;
                let name = name_by_id
                    .get(&pid)
                    .cloned()
                    .unwrap_or_else(|| pid.clone());
                (Some(pid), Some(m.confidence), name)
            } else {
                guest_n += 1;
                guests += 1;
                (None, Some(m.confidence), format!("Guest-{guest_n}"))
            }
        };

        identities.push(SpeakerIdentity {
            diar_label: label.clone(),
            display_name: display_name.clone(),
            person_id: person_id.clone(),
            confidence,
            speech_s,
        });

        for seg in segments.iter_mut() {
            if seg.speaker.as_deref() == Some(label.as_str()) {
                seg.speaker = Some(display_name.clone());
                seg.person_id = person_id.clone();
                seg.identify_confidence = confidence;
            }
        }
    }

    Ok(IdentifyResult {
        identities,
        matched,
        guests,
        skipped,
    })
}

#[cfg(not(feature = "diarization"))]
pub async fn identify_transcript(
    _audio_path: &Path,
    _transcript: &mut TranscriptionResponse,
    _storage: &MeetingStorage,
    _cfg: &DiarizeConfig,
    _threshold: f32,
) -> anyhow::Result<IdentifyResult> {
    anyhow::bail!("speaker identify requires the `diarization` feature")
}

/// Identify speakers on a stored meeting (latest transcript + recording).
///
/// Writes results to DB via [`MeetingStorage::apply_speaker_identities`].
#[cfg(feature = "diarization")]
pub async fn identify_meeting(
    storage: &MeetingStorage,
    meeting_id: &str,
    cfg: &DiarizeConfig,
    threshold: f32,
) -> anyhow::Result<(IdentifyResult, u64)> {
    let audio_path = storage.get_recording_path(meeting_id).await?;
    if !audio_path.exists() {
        anyhow::bail!("Recording not found for meeting: {meeting_id}");
    }
    let mut transcript = storage.get_transcript(meeting_id, None).await?;
    let result = identify_transcript(&audio_path, &mut transcript, storage, cfg, threshold).await?;

    let mut assignments: HashMap<String, (String, Option<String>, Option<f32>)> = HashMap::new();
    for id in &result.identities {
        assignments.insert(
            id.diar_label.clone(),
            (
                id.display_name.clone(),
                id.person_id.clone(),
                id.confidence,
            ),
        );
    }
    let updated = storage
        .apply_speaker_identities(meeting_id, &assignments)
        .await?;
    Ok((result, updated))
}

#[cfg(not(feature = "diarization"))]
pub async fn identify_meeting(
    _storage: &MeetingStorage,
    _meeting_id: &str,
    _cfg: &DiarizeConfig,
    _threshold: f32,
) -> anyhow::Result<(IdentifyResult, u64)> {
    anyhow::bail!("speaker identify requires the `diarization` feature")
}

/// Store a precomputed centroid (tests / external embedders).
pub async fn store_centroid(
    storage: &MeetingStorage,
    person_id: &str,
    model: &str,
    centroid: Vec<f32>,
    enrolled_from: VoiceprintEnrolledFrom,
) -> anyhow::Result<Voiceprint> {
    storage.get_person(person_id).await?;
    let dim = centroid.len() as u32;
    if dim == 0 {
        anyhow::bail!("centroid cannot be empty");
    }
    let now = Utc::now();
    let existing = storage.get_voiceprint(person_id).await?;
    let vp = Voiceprint {
        id: existing
            .as_ref()
            .map(|v| v.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        person_id: person_id.to_string(),
        model: model.to_string(),
        dim,
        centroid,
        enrolled_from,
        created_at: existing.as_ref().map(|v| v.created_at).unwrap_or(now),
        updated_at: now,
    };
    storage.upsert_voiceprint(&vp).await?;
    Ok(vp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Meeting, MeetingStatus, Person};
    use crate::transcription::{TranscriptSegment, TranscriptionResponse};
    use tempfile::TempDir;

    async fn setup() -> (TempDir, MeetingStorage) {
        let dir = TempDir::new().unwrap();
        let storage = MeetingStorage::in_memory(dir.path().to_path_buf())
            .await
            .unwrap();
        (dir, storage)
    }

    #[tokio::test]
    async fn store_centroid_and_match() {
        let (_dir, storage) = setup().await;
        let person = Person::new("Alice".to_string());
        storage.create_person(&person).await.unwrap();

        let mut centroid = vec![0.0f32; 8];
        centroid[0] = 1.0;
        store_centroid(
            &storage,
            &person.id,
            "test",
            centroid.clone(),
            VoiceprintEnrolledFrom::Sample,
        )
        .await
        .unwrap();

        #[cfg(feature = "diarization")]
        {
            let m = match_embedding(&storage, &centroid, 0.5).await.unwrap();
            assert_eq!(m.person_id.as_deref(), Some(person.id.as_str()));
            assert!(m.confidence > 0.99);

            let other = vec![0.0f32, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
            let m2 = match_embedding(&storage, &other, 0.9).await.unwrap();
            assert!(m2.person_id.is_none());
        }
    }

    #[tokio::test]
    async fn apply_speaker_identities_updates_db() {
        let (_dir, storage) = setup().await;
        let mut meeting = Meeting::new("ID".to_string());
        meeting.status = MeetingStatus::Ready;
        storage.create_meeting(&meeting).await.unwrap();

        let person = Person::new("Alice".to_string());
        storage.create_person(&person).await.unwrap();

        let segs = vec![
            TranscriptSegment {
                id: 0,
                start: 0.0,
                end: 5.0,
                text: "hi".into(),
                timestamp: None,
                tokens: None,
                temperature: None,
                avg_logprob: None,
                compression_ratio: None,
                no_speech_prob: None,
                speaker: Some("SPEAKER_00".into()),
                person_id: None,
                identify_confidence: None,
                refined_text: None,
            },
            TranscriptSegment {
                id: 1,
                start: 5.0,
                end: 10.0,
                text: "yo".into(),
                timestamp: None,
                tokens: None,
                temperature: None,
                avg_logprob: None,
                compression_ratio: None,
                no_speech_prob: None,
                speaker: Some("SPEAKER_01".into()),
                person_id: None,
                identify_confidence: None,
                refined_text: None,
            },
        ];
        let resp = TranscriptionResponse {
            text: "hi yo".into(),
            language: Some("en".into()),
            duration: Some(10.0),
            segments: Some(segs),
            refined_text: None,
        };
        storage
            .save_transcript(&meeting.id, &resp, "t", "m", 10)
            .await
            .unwrap();

        let mut map = HashMap::new();
        map.insert(
            "SPEAKER_00".to_string(),
            ("Alice".to_string(), Some(person.id.clone()), Some(0.88)),
        );
        map.insert(
            "SPEAKER_01".to_string(),
            ("Guest-1".to_string(), None, Some(0.2)),
        );
        let n = storage
            .apply_speaker_identities(&meeting.id, &map)
            .await
            .unwrap();
        assert_eq!(n, 2);

        let loaded = storage.get_transcript(&meeting.id, None).await.unwrap();
        let segs = loaded.segments.unwrap();
        assert_eq!(segs[0].speaker.as_deref(), Some("Alice"));
        assert_eq!(segs[0].person_id.as_deref(), Some(person.id.as_str()));
        assert_eq!(segs[1].speaker.as_deref(), Some("Guest-1"));
        assert!(segs[1].person_id.is_none());
    }
}
