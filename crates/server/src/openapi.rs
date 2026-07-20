//! OpenAPI documentation setup

use crate::config_handlers;
use crate::handlers;
use crate::import_handlers;
use crate::summary_handlers;
use crate::types::*;
use meeting_agent_core::jobs::{Job, JobState, JobType, ProgressEvent};
use meeting_agent_core::models::{
    FileMetadata, MatchedSegment, Meeting, MeetingSearchResult, MeetingStatus, MetadataSource,
    Summary, SummaryStatus, SummaryTemplate, Transcript, TranscriptSegment, TranscriptionInfo,
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
        handlers::get_transcript,
        handlers::search_all_transcripts,
        // Summary handlers
        summary_handlers::list_summaries,
        summary_handlers::get_summary,
        summary_handlers::create_summary,
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
    ),
    components(
        schemas(
            // Request types
            CreateMeetingRequest,
            UpdateMeetingRequest,
            CreateSummaryRequest,
            UpdateTranscriptionConfigRequest,
            UpdateSummaryConfigRequest,
            UpdateConfigRequest,
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
    )
)]
pub struct ApiDoc;
