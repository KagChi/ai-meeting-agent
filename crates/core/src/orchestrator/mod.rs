//! Live-bot orchestrator (Phase 4 v1).
//!
//! Flow: Vexa meeting ended → download recording → existing import pipeline.
//! Vexa live STT is not used; ASR runs via `runners::run_import_memory`.

mod config;
mod models;
mod service;
mod vexa;

pub use config::OrchestratorConfig;
pub use models::{
    parse_vexa_webhook, MeetingEndedEvent, OrchestratorImportRequest, OrchestratorRun,
    OrchestratorRunStatus, OrchestratorStartResult,
};
pub use service::{start_import_from_event, start_import_from_request};
pub use vexa::{DownloadedRecording, VexaClient};
