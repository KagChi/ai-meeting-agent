//! OpenAPI documentation setup

use crate::config_handlers;
use crate::handlers;
use crate::import_handlers;
use crate::person_handlers;
use crate::summary_handlers;
use crate::types::*;
use meeting_agent_core::jobs::{Job, JobState, JobType, ProgressEvent};
use meeting_agent_core::models::{
    FileMetadata, MatchedSegment, Meeting, MeetingSearchResult, MeetingStatus, MetadataSource,
    Person, Summary, SummaryStatus, SummaryTemplate, Transcript, TranscriptSegment,
    TranscriptionInfo, VoiceprintSample, VoiceprintSampleSource,
};
use meeting_agent_core::transcription::TranscriptionResponse;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        // Core handlers
        handlers::health,
        handlers::version,
        handlers::list_meetings,
        handlers::get_meeting,
        handlers::create_meeting,
        handlers::update_meeting,
        handlers::delete_meeting,
        handlers::rename_speakers,
        handlers::identify_speakers,
        handlers::get_transcript,
        handlers::search_all_transcripts,
        // Summary handlers
        summary_handlers::list_summaries,
        summary_handlers::get_summary,
        summary_handlers::create_summary,
        summary_handlers::update_summary,
        // Import handlers
        import_handlers::create_import,
        import_handlers::validate_import,
        import_handlers::get_import_status,
        import_handlers::cancel_import,
        // Config handlers
        config_handlers::get_config,
        config_handlers::update_config,
        config_handlers::get_transcription_config,
        config_handlers::update_transcription_config,
        config_handlers::get_summary_config,
        config_handlers::update_summary_config,
        // Voice bank
        person_handlers::list_persons,
        person_handlers::create_person,
        person_handlers::get_person,
        person_handlers::update_person,
        person_handlers::delete_person,
        person_handlers::list_samples,
        person_handlers::add_sample,
        person_handlers::delete_sample,
        person_handlers::rebuild_person_voiceprint,
        person_handlers::list_voiceprints,
    ),
    components(
        schemas(
            // Request types
            CreateMeetingRequest,
            UpdateMeetingRequest,
            RenameSpeakersRequest,
            RenameSpeakersResponse,
            IdentifySpeakersResponse,
            SpeakerIdentityResponse,
            CreateSummaryRequest,
            UpdateSummaryRequest,
            UpdateTranscriptionConfigRequest,
            UpdateSummaryConfigRequest,
            UpdateConfigRequest,
            CreatePersonRequest,
            UpdatePersonRequest,
            // Response types
            ListMeetingsResponse,
            MeetingResponse,
            TranscriptResponse,
            SearchTranscriptsResponse,
            ErrorResponse,
            ImportResponse,
            JobStatusResponse,
            ImportValidationResponse,
            CancelImportResponse,
            SummaryResponse,
            ListSummariesResponse,
            CreateSummaryResponse,
            TranscriptionConfigResponse,
            SummaryConfigResponse,
            ConfigResponse,
            PersonResponse,
            ListPersonsResponse,
            VoiceprintSampleResponse,
            ListVoiceprintSamplesResponse,
            VoiceprintMetaResponse,
            ListVoiceprintsResponse,
            RebuildVoiceprintResponse,
            // Core domain models
            Meeting,
            MeetingStatus,
            MetadataSource,
            FileMetadata,
            TranscriptionInfo,
            Transcript,
            TranscriptSegment,
            MatchedSegment,
            MeetingSearchResult,
            Summary,
            SummaryTemplate,
            SummaryStatus,
            TranscriptionResponse,
            Job,
            JobType,
            JobState,
            ProgressEvent,
            Person,
            VoiceprintSample,
            VoiceprintSampleSource,
        )
    ),
    info(
        title = "Meeting Agent API",
        version = "0.1.0",
        description = "REST API for meeting transcription, summarization, and management",
    ),
    tags(
        (name = "meetings", description = "Meeting management endpoints"),
        (name = "transcripts", description = "Transcript retrieval endpoints"),
        (name = "summaries", description = "Summary generation and retrieval endpoints"),
        (name = "imports", description = "Audio import and processing endpoints"),
        (name = "jobs", description = "Background job status and control endpoints"),
        (name = "config", description = "Configuration management endpoints"),
        (name = "persons", description = "Voice bank: persons, enrollment samples, voiceprints"),
    )
)]
pub struct ApiDoc;
