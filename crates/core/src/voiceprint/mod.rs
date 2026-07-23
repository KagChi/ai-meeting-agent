//! Voice bank: rebuild centroids from enrollment samples + identify match.

use std::collections::HashMap;
#[cfg(feature = "diarization")]
use std::collections::HashSet;
use std::path::Path;

use chrono::Utc;
use uuid::Uuid;

use crate::config::DiarizeConfig;
#[cfg(feature = "diarization")]
use crate::models::{Person, VoiceprintSampleSource};
use crate::models::{Voiceprint, VoiceprintEnrolledFrom};
use crate::storage::MeetingStorage;
use crate::transcription::TranscriptionResponse;

#[cfg(feature = "diarization")]
use crate::diarize::embed::{cosine_similarity, embed_audio_file, mean_pool_l2};

/// Default minimum total enrolled speech (seconds) before building a centroid.
pub const DEFAULT_ENROLL_MIN_SPEECH_S: f64 = 30.0;

/// Minimum speech (seconds) before auto-enrolling a guest as a new person + sample.
pub const DEFAULT_AUTO_ENROLL_MIN_SPEECH_S: f64 = 10.0;

/// Max speech (seconds) written into an auto-enroll sample WAV (centroid uses full query).
pub const DEFAULT_AUTO_ENROLL_SAMPLE_MAX_S: f64 = 30.0;

/// Default cosine threshold for person match (tuned later on lab voices).
pub const DEFAULT_IDENTIFY_THRESHOLD: f32 = 0.55;

/// Cosine threshold to merge two unmatched diarization clusters within one meeting.
///
/// Must be **stricter** than [`DEFAULT_IDENTIFY_THRESHOLD`]: bank match at 0.55 is for
/// known people; within-meeting merge only joins diarization over-splits of the *same*
/// talker. At 0.55, different speakers in the same room often collapse into one Guest.
pub const DEFAULT_WITHIN_MEETING_MERGE_THRESHOLD: f32 = 0.85;

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
            log::warn!("[voiceprint] sample file missing: {}; skip", abs.display());
            continue;
        }
        match embed_audio_file(&abs, None, None, cfg) {
            Ok(vec) => embeddings.push(vec),
            Err(e) => {
                log::warn!("[voiceprint] embed failed for {}: {e:#}", abs.display());
            }
        }
    }

    if embeddings.is_empty() {
        anyhow::bail!("no samples could be embedded for person {person_id}");
    }

    let centroid = crate::diarize::embed::mean_pool_l2(&embeddings)?;
    let expected_dim = cfg.embedding_dim as usize;
    if centroid.len() != expected_dim {
        anyhow::bail!(
            "unexpected embed dim {} (expected {expected_dim} for model {})",
            centroid.len(),
            cfg.embedding_model
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
        model: cfg.embedding_model.clone(),
        dim: cfg.embedding_dim,
        centroid,
        enrolled_from: VoiceprintEnrolledFrom::Sample,
        created_at: existing.as_ref().map(|v| v.created_at).unwrap_or(now),
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
///
/// When `model_id` is set, only voiceprints from that embedding model are
/// considered (prevents mixing ResNet34 256-d with CAM++ 512-d).
#[cfg(feature = "diarization")]
pub async fn match_embedding(
    storage: &MeetingStorage,
    query: &[f32],
    threshold: f32,
    model_id: Option<&str>,
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
        if let Some(mid) = model_id {
            if vp.model != mid {
                continue;
            }
        }
        if vp.centroid.len() != query.len() || vp.dim as usize != query.len() {
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
    _model_id: Option<&str>,
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

/// Next free `Guest-N` index from existing person names.
#[cfg(feature = "diarization")]
fn next_guest_index(name_by_id: &HashMap<String, String>) -> u32 {
    let mut max_n = 0u32;
    for name in name_by_id.values() {
        if let Some(rest) = name.strip_prefix("Guest-") {
            if let Ok(n) = rest.parse::<u32>() {
                max_n = max_n.max(n);
            }
        }
    }
    max_n + 1
}

/// Take spans in time order until total duration reaches `max_s` (trim last span).
#[cfg(any(feature = "diarization", test))]
fn cap_spans_to_duration(spans: &[(f64, f64)], max_s: f64) -> (Vec<(f64, f64)>, f64) {
    if max_s <= 0.0 || spans.is_empty() {
        return (Vec::new(), 0.0);
    }
    let mut ordered: Vec<(f64, f64)> = spans.iter().copied().filter(|(a, b)| b - a > 0.0).collect();
    ordered.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut out = Vec::new();
    let mut taken = 0.0;
    for (start, end) in ordered {
        if taken + f64::EPSILON >= max_s {
            break;
        }
        let span_len = (end - start).max(0.0);
        if span_len <= 0.0 {
            continue;
        }
        let remain = max_s - taken;
        if span_len <= remain + f64::EPSILON {
            out.push((start, end));
            taken += span_len;
        } else {
            out.push((start, start + remain));
            taken += remain;
            break;
        }
    }
    (out, taken)
}

/// Union-find parent for within-meeting cluster merge.
#[cfg(any(feature = "diarization", test))]
fn uf_find(parent: &mut [usize], i: usize) -> usize {
    let mut i = i;
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

#[cfg(any(feature = "diarization", test))]
fn uf_union(parent: &mut [usize], a: usize, b: usize) {
    let ra = uf_find(parent, a);
    let rb = uf_find(parent, b);
    if ra != rb {
        parent[rb] = ra;
    }
}

/// Create person + meeting-turn sample (+ optional centroid) for an unmatched speaker.
#[cfg(feature = "diarization")]
#[allow(clippy::too_many_arguments)]
async fn auto_enroll_guest(
    storage: &MeetingStorage,
    audio_path: &Path,
    meeting_id: Option<&str>,
    spans: &[(f64, f64)],
    speech_s: f64,
    query: &[f32],
    display_name: &str,
    segment_ids: &[u32],
    model_id: &str,
) -> anyhow::Result<String> {
    let person = Person::new(display_name.to_string());
    storage.create_person(&person).await?;

    match crate::audio::extract_spans_to_wav_bytes(audio_path, spans) {
        Ok(bytes) => {
            if let Err(e) = storage
                .add_voiceprint_sample(
                    &person.id,
                    &bytes,
                    speech_s,
                    VoiceprintSampleSource::MeetingTurn,
                    meeting_id,
                    segment_ids,
                )
                .await
            {
                log::warn!(
                    "[identify] auto-enroll sample save failed for {}: {e:#}",
                    person.id
                );
            }
        }
        Err(e) => {
            log::warn!(
                "[identify] auto-enroll extract failed for {}: {e:#}",
                display_name
            );
        }
    }

    // Store centroid from the identify query so future meetings can match immediately.
    if let Err(e) = store_centroid(
        storage,
        &person.id,
        model_id,
        query.to_vec(),
        VoiceprintEnrolledFrom::MeetingTurn,
    )
    .await
    {
        log::warn!(
            "[identify] auto-enroll centroid failed for {}: {e:#}",
            person.id
        );
    } else {
        log::info!(
            "[identify] auto-enrolled {display_name} ({}) speech={speech_s:.1}s model={model_id}",
            person.id
        );
    }

    Ok(person.id)
}

/// Identify diarized speakers in a transcript against the voice bank.
///
/// For each unique non-empty `speaker` label:
/// 1. Collect segment time spans
/// 2. Embed those spans from `audio_path` (mean-pool)
/// 3. Cosine-match vs enrolled centroids
/// 4. ≥ threshold → person name + `person_id`
/// 5. else merge similar unmatched clusters within this meeting, then
///    auto-enroll as `Guest-N` when speech ≥ [`DEFAULT_AUTO_ENROLL_MIN_SPEECH_S`]
///    (sample capped at [`DEFAULT_AUTO_ENROLL_SAMPLE_MAX_S`]); short speech stays label-only
///
/// Mutates `transcript.segments` speaker/person_id/confidence in place.
/// Does **not** write segment renames to DB — caller applies via storage.
#[cfg(feature = "diarization")]
pub async fn identify_transcript(
    audio_path: &Path,
    transcript: &mut TranscriptionResponse,
    storage: &MeetingStorage,
    cfg: &DiarizeConfig,
    threshold: f32,
) -> anyhow::Result<IdentifyResult> {
    identify_transcript_with_meeting(audio_path, transcript, storage, cfg, threshold, None).await
}

/// Per-label work item after embed (before bank match / enroll).
#[cfg(feature = "diarization")]
struct LabelEmbed {
    label: String,
    spans: Vec<(f64, f64)>,
    segment_ids: Vec<u32>,
    speech_s: f64,
    query: Vec<f32>,
}

/// Same as [`identify_transcript`] but tags auto-enrolled samples with `meeting_id`.
#[cfg(feature = "diarization")]
pub async fn identify_transcript_with_meeting(
    audio_path: &Path,
    transcript: &mut TranscriptionResponse,
    storage: &MeetingStorage,
    cfg: &DiarizeConfig,
    threshold: f32,
    meeting_id: Option<&str>,
) -> anyhow::Result<IdentifyResult> {
    let persons = storage.list_persons().await?;
    let mut name_by_id: HashMap<String, String> =
        persons.into_iter().map(|p| (p.id, p.name)).collect();

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

    let mut embedded: Vec<LabelEmbed> = Vec::new();
    let mut skipped = 0u32;

    for label in &labels {
        let spans: Vec<(f64, f64)> = segments
            .iter()
            .filter(|s| s.speaker.as_deref() == Some(label.as_str()))
            .map(|s| (s.start, s.end))
            .collect();
        let segment_ids: Vec<u32> = segments
            .iter()
            .filter(|s| s.speaker.as_deref() == Some(label.as_str()))
            .map(|s| s.id)
            .collect();
        let speech_s: f64 = spans.iter().map(|(a, b)| (b - a).max(0.0)).sum();

        if speech_s < 0.5 {
            skipped += 1;
            log::info!("[identify] skip {label}: only {speech_s:.2}s speech");
            continue;
        }

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
        embedded.push(LabelEmbed {
            label: label.clone(),
            spans,
            segment_ids,
            speech_s,
            query,
        });
    }

    // Bank match first; collect unmatched indices for within-meeting merge.
    let mut bank_hit: Vec<Option<(String, f32, String)>> = vec![None; embedded.len()];
    let mut unmatched_idx: Vec<usize> = Vec::new();

    for (i, item) in embedded.iter().enumerate() {
        let m =
            match_embedding(storage, &item.query, threshold, Some(&cfg.embedding_model)).await?;
        if let Some(pid) = m.person_id {
            let name = name_by_id.get(&pid).cloned().unwrap_or_else(|| pid.clone());
            bank_hit[i] = Some((pid, m.confidence, name));
        } else {
            unmatched_idx.push(i);
        }
    }

    // Merge similar unmatched diar clusters (same person split by diarization).
    let mut parent: Vec<usize> = (0..embedded.len()).collect();
    let merge_thr = DEFAULT_WITHIN_MEETING_MERGE_THRESHOLD;
    for a in 0..unmatched_idx.len() {
        for b in (a + 1)..unmatched_idx.len() {
            let i = unmatched_idx[a];
            let j = unmatched_idx[b];
            let score = cosine_similarity(&embedded[i].query, &embedded[j].query);
            if score >= merge_thr {
                log::info!(
                    "[identify] within-meeting merge {} ~ {} (cos={score:.3} thr={merge_thr})",
                    embedded[i].label,
                    embedded[j].label
                );
                uf_union(&mut parent, i, j);
            } else {
                log::debug!(
                    "[identify] no merge {} ~ {} (cos={score:.3} < thr={merge_thr})",
                    embedded[i].label,
                    embedded[j].label
                );
            }
        }
    }

    // Group unmatched by merge root; process groups in first-seen root order.
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for &i in &unmatched_idx {
        let r = uf_find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut group_roots: Vec<usize> = groups.keys().copied().collect();
    group_roots.sort_unstable();

    // Assignment per embedded index: (person_id, confidence, display_name)
    type SpeakerAssign = (Option<String>, Option<f32>, String);
    let mut assign: Vec<Option<SpeakerAssign>> = vec![None; embedded.len()];
    let mut matched = 0u32;
    let mut guests = 0u32;
    let mut guest_n = next_guest_index(&name_by_id);

    for (i, hit) in bank_hit.iter().enumerate() {
        if let Some((pid, conf, name)) = hit {
            matched += 1;
            assign[i] = Some((Some(pid.clone()), Some(*conf), name.clone()));
        }
    }

    for root in group_roots {
        let members = groups.get(&root).cloned().unwrap_or_default();
        if members.is_empty() {
            continue;
        }
        // Prefer longest speech as enroll representative.
        let rep = *members
            .iter()
            .max_by(|a, b| {
                embedded[**a]
                    .speech_s
                    .partial_cmp(&embedded[**b].speech_s)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(&members[0]);

        let mut all_spans: Vec<(f64, f64)> = Vec::new();
        let mut all_seg_ids: Vec<u32> = Vec::new();
        let mut total_speech = 0.0;
        for &mi in &members {
            all_spans.extend(embedded[mi].spans.iter().copied());
            all_seg_ids.extend(embedded[mi].segment_ids.iter().copied());
            total_speech += embedded[mi].speech_s;
        }
        all_spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        all_seg_ids.sort_unstable();
        all_seg_ids.dedup();

        let (person_id, confidence, display_name) = if total_speech + f64::EPSILON
            >= DEFAULT_AUTO_ENROLL_MIN_SPEECH_S
        {
            let name = format!("Guest-{guest_n}");
            guest_n += 1;
            guests += 1;
            let (sample_spans, sample_s) =
                cap_spans_to_duration(&all_spans, DEFAULT_AUTO_ENROLL_SAMPLE_MAX_S);
            let enroll_spans = if sample_spans.is_empty() {
                embedded[rep].spans.clone()
            } else {
                sample_spans
            };
            let enroll_dur = if sample_s > 0.0 {
                sample_s
            } else {
                embedded[rep].speech_s.min(DEFAULT_AUTO_ENROLL_SAMPLE_MAX_S)
            };
            match auto_enroll_guest(
                storage,
                audio_path,
                meeting_id,
                &enroll_spans,
                enroll_dur,
                &embedded[rep].query,
                &name,
                &all_seg_ids,
                &cfg.embedding_model,
            )
            .await
            {
                Ok(pid) => {
                    name_by_id.insert(pid.clone(), name.clone());
                    // Auto-enroll is not a bank match score.
                    (Some(pid), None, name)
                }
                Err(e) => {
                    log::warn!(
                        "[identify] auto-enroll failed for {}: {e:#}",
                        embedded[rep].label
                    );
                    (None, None, name)
                }
            }
        } else {
            let name = format!("Guest-{guest_n}");
            guest_n += 1;
            guests += 1;
            log::info!(
                    "[identify] {}: speech {total_speech:.1}s < auto-enroll min {DEFAULT_AUTO_ENROLL_MIN_SPEECH_S}s; label only",
                    embedded[rep].label
                );
            (None, None, name)
        };

        for &mi in &members {
            assign[mi] = Some((person_id.clone(), confidence, display_name.clone()));
        }
    }

    let mut identities = Vec::new();
    for (i, item) in embedded.iter().enumerate() {
        let Some((person_id, confidence, display_name)) = assign[i].clone() else {
            skipped += 1;
            continue;
        };

        identities.push(SpeakerIdentity {
            diar_label: item.label.clone(),
            display_name: display_name.clone(),
            person_id: person_id.clone(),
            confidence,
            speech_s: item.speech_s,
        });

        for seg in segments.iter_mut() {
            if seg.speaker.as_deref() == Some(item.label.as_str()) {
                // Keep speaker unchanged (raw diarization label)
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

#[cfg(not(feature = "diarization"))]
pub async fn identify_transcript_with_meeting(
    _audio_path: &Path,
    _transcript: &mut TranscriptionResponse,
    _storage: &MeetingStorage,
    _cfg: &DiarizeConfig,
    _threshold: f32,
    _meeting_id: Option<&str>,
) -> anyhow::Result<IdentifyResult> {
    anyhow::bail!("speaker identify requires the `diarization` feature")
}

/// Identify speakers on a stored meeting (latest transcript + recording).
///
/// Unmatched speakers with enough speech are auto-enrolled into the voice bank.
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
    let result = identify_transcript_with_meeting(
        &audio_path,
        &mut transcript,
        storage,
        cfg,
        threshold,
        Some(meeting_id),
    )
    .await?;

    let mut assignments: HashMap<String, (String, Option<String>, Option<f32>)> = HashMap::new();
    for id in &result.identities {
        assignments.insert(
            id.diar_label.clone(),
            (id.display_name.clone(), id.person_id.clone(), id.confidence),
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
            let m = match_embedding(&storage, &centroid, 0.5, Some("test"))
                .await
                .unwrap();
            assert_eq!(m.person_id.as_deref(), Some(person.id.as_str()));
            assert!(m.confidence > 0.99);

            let other = vec![0.0f32, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
            let m2 = match_embedding(&storage, &other, 0.9, Some("test"))
                .await
                .unwrap();
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
        let guest = Person::new("Guest-1".to_string());
        storage.create_person(&guest).await.unwrap();

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
                display_name: None,
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
                display_name: None,
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
            ("Guest-1".to_string(), Some(guest.id.clone()), Some(0.2)),
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
        assert_eq!(segs[1].person_id.as_deref(), Some(guest.id.as_str()));
    }

    #[test]
    fn next_guest_index_from_names() {
        let mut m = HashMap::new();
        m.insert("a".into(), "Alice".into());
        m.insert("b".into(), "Guest-3".into());
        m.insert("c".into(), "Guest-1".into());
        assert_eq!(next_guest_index(&m), 4);
        assert_eq!(next_guest_index(&HashMap::new()), 1);
    }

    #[test]
    fn cap_spans_respects_max_duration() {
        let spans = vec![(0.0, 20.0), (30.0, 50.0), (60.0, 80.0)];
        let (capped, dur) = cap_spans_to_duration(&spans, 30.0);
        assert!((dur - 30.0).abs() < 1e-6);
        assert_eq!(capped.len(), 2);
        assert_eq!(capped[0], (0.0, 20.0));
        assert!((capped[1].1 - capped[1].0 - 10.0).abs() < 1e-6);
        assert!((capped[1].0 - 30.0).abs() < 1e-6);
    }

    #[test]
    fn cap_spans_empty_or_zero_max() {
        assert_eq!(cap_spans_to_duration(&[], 30.0).1, 0.0);
        assert_eq!(cap_spans_to_duration(&[(0.0, 5.0)], 0.0).0.len(), 0);
    }

    #[test]
    fn within_meeting_uf_merges_pairs() {
        let mut parent = vec![0, 1, 2];
        uf_union(&mut parent, 0, 2);
        assert_eq!(uf_find(&mut parent, 0), uf_find(&mut parent, 2));
        assert_ne!(uf_find(&mut parent, 0), uf_find(&mut parent, 1));
    }
}
