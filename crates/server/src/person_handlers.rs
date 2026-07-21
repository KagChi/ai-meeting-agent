//! Voice bank: persons, enrollment samples, voiceprint metadata.

use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    Json,
};
use meeting_agent_core::models::{Person, Voiceprint, VoiceprintSampleSource};
use meeting_agent_core::voiceprint::{rebuild_voiceprint, DEFAULT_ENROLL_MIN_SPEECH_S};

use crate::error::ApiError;
use crate::state::AppState;
use crate::types::{
    CreatePersonRequest, ListPersonsResponse, ListVoiceprintSamplesResponse,
    ListVoiceprintsResponse, PersonResponse, RebuildVoiceprintResponse, UpdatePersonRequest,
    VoiceprintMetaResponse, VoiceprintSampleResponse,
};
use crate::validation::{validate_person_aliases, validate_person_name, validate_uuid};

fn voiceprint_meta(vp: Voiceprint) -> VoiceprintMetaResponse {
    VoiceprintMetaResponse {
        id: vp.id,
        person_id: vp.person_id,
        model: vp.model,
        dim: vp.dim,
        enrolled_from: match vp.enrolled_from {
            meeting_agent_core::models::VoiceprintEnrolledFrom::Sample => "sample".to_string(),
            meeting_agent_core::models::VoiceprintEnrolledFrom::MeetingTurn => {
                "meeting_turn".to_string()
            }
        },
        created_at: vp.created_at,
        updated_at: vp.updated_at,
    }
}

#[utoipa::path(
    get,
    path = "/persons",
    tag = "persons",
    responses(
        (status = 200, description = "List enrolled persons", body = ListPersonsResponse),
    )
)]
pub async fn list_persons(
    State(state): State<AppState>,
) -> Result<Json<ListPersonsResponse>, ApiError> {
    let persons = state.storage.list_persons().await?;
    let total = persons.len() as u64;
    Ok(Json(ListPersonsResponse { persons, total }))
}

#[utoipa::path(
    post,
    path = "/persons",
    tag = "persons",
    request_body = CreatePersonRequest,
    responses(
        (status = 201, description = "Person created", body = PersonResponse),
        (status = 400, description = "Invalid request", body = crate::types::ErrorResponse),
    )
)]
pub async fn create_person(
    State(state): State<AppState>,
    Json(req): Json<CreatePersonRequest>,
) -> Result<(StatusCode, Json<PersonResponse>), ApiError> {
    validate_person_name(&req.name)?;
    validate_person_aliases(&req.aliases)?;

    let mut person = Person::new(req.name.trim().to_string());
    person.aliases = req
        .aliases
        .into_iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();

    state.storage.create_person(&person).await?;
    Ok((StatusCode::CREATED, Json(PersonResponse { person })))
}

#[utoipa::path(
    get,
    path = "/persons/{id}",
    tag = "persons",
    params(("id" = String, Path, description = "Person UUID")),
    responses(
        (status = 200, description = "Person found", body = PersonResponse),
        (status = 404, description = "Not found", body = crate::types::ErrorResponse),
    )
)]
pub async fn get_person(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PersonResponse>, ApiError> {
    validate_uuid(&id)?;
    let person = state.storage.get_person(&id).await?;
    Ok(Json(PersonResponse { person }))
}

#[utoipa::path(
    patch,
    path = "/persons/{id}",
    tag = "persons",
    params(("id" = String, Path, description = "Person UUID")),
    request_body = UpdatePersonRequest,
    responses(
        (status = 200, description = "Person updated", body = PersonResponse),
        (status = 400, description = "Invalid request", body = crate::types::ErrorResponse),
        (status = 404, description = "Not found", body = crate::types::ErrorResponse),
    )
)]
pub async fn update_person(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdatePersonRequest>,
) -> Result<Json<PersonResponse>, ApiError> {
    validate_uuid(&id)?;
    if req.name.is_none() && req.aliases.is_none() {
        return Err(ApiError::BadRequest(
            "At least one of name or aliases must be provided".to_string(),
        ));
    }

    let mut person = state.storage.get_person(&id).await?;
    if let Some(name) = req.name {
        validate_person_name(&name)?;
        person.name = name.trim().to_string();
    }
    if let Some(aliases) = req.aliases {
        validate_person_aliases(&aliases)?;
        person.aliases = aliases
            .into_iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect();
    }

    state
        .storage
        .update_person(&id, &person.name, &person.aliases)
        .await?;
    let person = state.storage.get_person(&id).await?;
    Ok(Json(PersonResponse { person }))
}

#[utoipa::path(
    delete,
    path = "/persons/{id}",
    tag = "persons",
    params(("id" = String, Path, description = "Person UUID")),
    responses(
        (status = 204, description = "Person deleted"),
        (status = 404, description = "Not found", body = crate::types::ErrorResponse),
    )
)]
pub async fn delete_person(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    validate_uuid(&id)?;
    state.storage.delete_person(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/persons/{id}/samples",
    tag = "persons",
    params(("id" = String, Path, description = "Person UUID")),
    responses(
        (status = 200, description = "Enrollment samples", body = ListVoiceprintSamplesResponse),
        (status = 404, description = "Person not found", body = crate::types::ErrorResponse),
    )
)]
pub async fn list_samples(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ListVoiceprintSamplesResponse>, ApiError> {
    validate_uuid(&id)?;
    let _ = state.storage.get_person(&id).await?;
    let samples = state.storage.list_voiceprint_samples(&id).await?;
    let total = samples.len() as u64;
    Ok(Json(ListVoiceprintSamplesResponse { samples, total }))
}

#[utoipa::path(
    post,
    path = "/persons/{id}/samples",
    tag = "persons",
    params(("id" = String, Path, description = "Person UUID")),
    responses(
        (status = 201, description = "Sample stored", body = VoiceprintSampleResponse),
        (status = 400, description = "Invalid request", body = crate::types::ErrorResponse),
        (status = 404, description = "Person not found", body = crate::types::ErrorResponse),
    )
)]
pub async fn add_sample(
    State(state): State<AppState>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<VoiceprintSampleResponse>), ApiError> {
    validate_uuid(&id)?;
    let _ = state.storage.get_person(&id).await?;

    let mut audio_bytes: Option<Vec<u8>> = None;
    let mut duration_s: Option<f64> = None;
    let mut meeting_id: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("Failed to read multipart field: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError::BadRequest(format!("Failed to read file bytes: {e}")))?;
                if bytes.is_empty() {
                    return Err(ApiError::BadRequest("Audio file is empty".to_string()));
                }
                audio_bytes = Some(bytes.to_vec());
            }
            "duration_s" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| ApiError::BadRequest(format!("Failed to read duration_s: {e}")))?;
                let v: f64 = text.trim().parse().map_err(|_| {
                    ApiError::BadRequest(format!("Invalid duration_s: {text}"))
                })?;
                if v < 0.0 {
                    return Err(ApiError::BadRequest(
                        "duration_s must be non-negative".to_string(),
                    ));
                }
                duration_s = Some(v);
            }
            "meeting_id" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| ApiError::BadRequest(format!("Failed to read meeting_id: {e}")))?;
                let t = text.trim();
                if !t.is_empty() {
                    validate_uuid(t)?;
                    meeting_id = Some(t.to_string());
                }
            }
            _ => {}
        }
    }

    let audio_bytes =
        audio_bytes.ok_or_else(|| ApiError::BadRequest("Missing 'file' field".to_string()))?;
    let duration_s = duration_s.unwrap_or(0.0);

    let sample = state
        .storage
        .add_voiceprint_sample(
            &id,
            &audio_bytes,
            duration_s,
            VoiceprintSampleSource::Upload,
            meeting_id.as_deref(),
            &[],
        )
        .await?;

    // Best-effort centroid rebuild (needs diarization feature + enough speech).
    let cfg = state.config.read().await.diarize.clone();
    if let Err(e) = rebuild_voiceprint(
        &state.storage,
        &id,
        &cfg,
        DEFAULT_ENROLL_MIN_SPEECH_S,
    )
    .await
    {
        log::warn!("[persons] rebuild after sample add failed: {e:#}");
    }

    Ok((
        StatusCode::CREATED,
        Json(VoiceprintSampleResponse { sample }),
    ))
}

#[utoipa::path(
    post,
    path = "/persons/{id}/voiceprint/rebuild",
    tag = "persons",
    params(("id" = String, Path, description = "Person UUID")),
    responses(
        (status = 200, description = "Rebuild attempted", body = RebuildVoiceprintResponse),
        (status = 404, description = "Person not found", body = crate::types::ErrorResponse),
    )
)]
pub async fn rebuild_person_voiceprint(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RebuildVoiceprintResponse>, ApiError> {
    validate_uuid(&id)?;
    let _ = state.storage.get_person(&id).await?;
    let cfg = state.config.read().await.diarize.clone();
    match rebuild_voiceprint(&state.storage, &id, &cfg, DEFAULT_ENROLL_MIN_SPEECH_S).await {
        Ok(Some(vp)) => Ok(Json(RebuildVoiceprintResponse {
            rebuilt: true,
            voiceprint: Some(voiceprint_meta(vp)),
            message: None,
        })),
        Ok(None) => Ok(Json(RebuildVoiceprintResponse {
            rebuilt: false,
            voiceprint: None,
            message: Some(format!(
                "Not enough enrolled speech (need ≥ {DEFAULT_ENROLL_MIN_SPEECH_S}s total) or no samples"
            )),
        })),
        Err(e) => Err(ApiError::from(e)),
    }
}

#[utoipa::path(
    delete,
    path = "/persons/{person_id}/samples/{sample_id}",
    tag = "persons",
    params(
        ("person_id" = String, Path, description = "Person UUID"),
        ("sample_id" = String, Path, description = "Sample UUID"),
    ),
    responses(
        (status = 204, description = "Sample deleted"),
        (status = 404, description = "Not found", body = crate::types::ErrorResponse),
    )
)]
pub async fn delete_sample(
    State(state): State<AppState>,
    Path((person_id, sample_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    validate_uuid(&person_id)?;
    validate_uuid(&sample_id)?;
    let _ = state.storage.get_person(&person_id).await?;

    let samples = state.storage.list_voiceprint_samples(&person_id).await?;
    if !samples.iter().any(|s| s.id == sample_id) {
        return Err(ApiError::NotFound(format!(
            "Voiceprint sample not found: {sample_id}"
        )));
    }

    state.storage.delete_voiceprint_sample(&sample_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/voiceprints",
    tag = "persons",
    responses(
        (status = 200, description = "Enrolled voiceprint metadata", body = ListVoiceprintsResponse),
    )
)]
pub async fn list_voiceprints(
    State(state): State<AppState>,
) -> Result<Json<ListVoiceprintsResponse>, ApiError> {
    let vps = state.storage.list_voiceprints().await?;
    let voiceprints: Vec<VoiceprintMetaResponse> =
        vps.into_iter().map(voiceprint_meta).collect();
    let total = voiceprints.len() as u64;
    Ok(Json(ListVoiceprintsResponse {
        voiceprints,
        total,
    }))
}
