//! Integration tests for metadata in API responses

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::NaiveDateTime;
use meeting_agent_core::models::{FileMetadata, Meeting, MeetingStatus, MetadataSource};
use meeting_agent_core::{Config, JobRegistry, MeetingStorage};
use meeting_agent_server::AppState;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;

/// Helper to build test AppState with temp storage and config
fn test_app_state() -> (AppState, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = MeetingStorage::with_base(temp_dir.path().to_path_buf());

    let config_path = temp_dir.path().join("config.json");
    let mut config = Config::default();
    config.server.api_key = Some("test-key".to_string());
    config.save(&config_path).unwrap();

    let state = AppState {
        config: Arc::new(RwLock::new(config)),
        config_path,
        storage: Arc::new(storage),
        jobs: Arc::new(JobRegistry::new()),
    };

    (state, temp_dir)
}

#[tokio::test]
async fn test_get_meeting_includes_metadata() {
    let (state, _temp_dir) = test_app_state();

    // Create meeting with metadata
    let mut meeting = Meeting::new("Test Meeting with Metadata".to_string());
    meeting.status = MeetingStatus::Ready;
    meeting.participants = Some(vec![
        "Alice".to_string(),
        "Bob".to_string(),
        "Charlie".to_string(),
    ]);
    meeting.location = Some("Conference Room A".to_string());
    meeting.organizer = Some("Alice".to_string());
    meeting.metadata_source = Some(MetadataSource::Filename);
    meeting.file_metadata = Some(FileMetadata {
        codec: Some("aac".to_string()),
        sample_rate: Some(44100),
        bit_rate: Some(128000),
        channels: Some(2),
        file_size_bytes: Some(5242880),
    });
    meeting.recording_date = Some(NaiveDateTime::parse_from_str(
        "2024-03-15 09:00:00",
        "%Y-%m-%d %H:%M:%S",
    ).unwrap());
    meeting.audio_file = Some("meeting.m4a".to_string());

    let meeting_id = meeting.id.clone();
    state.storage.create_meeting(&meeting).unwrap();

    let app = meeting_agent_server::build_router(state);

    let request = Request::builder()
        .uri(format!("/meetings/{}", meeting_id))
        .method("GET")
        .header("X-API-Key", "test-key")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    // Verify metadata fields are present
    assert_eq!(json["id"], meeting_id);
    assert_eq!(json["title"], "Test Meeting with Metadata");
    assert_eq!(json["participants"][0], "Alice");
    assert_eq!(json["participants"][1], "Bob");
    assert_eq!(json["participants"][2], "Charlie");
    assert_eq!(json["location"], "Conference Room A");
    assert_eq!(json["organizer"], "Alice");
    assert_eq!(json["metadata_source"], "filename");
    assert_eq!(json["file_metadata"]["codec"], "aac");
    assert_eq!(json["file_metadata"]["sample_rate"], 44100);
    assert_eq!(json["file_metadata"]["bit_rate"], 128000);
    assert_eq!(json["file_metadata"]["channels"], 2);
    assert_eq!(json["file_metadata"]["file_size_bytes"], 5242880);
    assert!(json["recording_date"].is_string());
    assert_eq!(json["audio_file"], "meeting.m4a");
}

#[tokio::test]
async fn test_post_meeting_response_includes_metadata_fields() {
    let (state, _temp_dir) = test_app_state();

    let app = meeting_agent_server::build_router(state);

    let request_body = r#"{
        "title": "New Meeting",
        "date": "2024-03-15T09:00:00Z"
    }"#;

    let request = Request::builder()
        .uri("/meetings")
        .method("POST")
        .header("X-API-Key", "test-key")
        .header("content-type", "application/json")
        .body(Body::from(request_body))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    // Verify response structure includes metadata fields (even if null)
    assert!(json["id"].is_string());
    assert_eq!(json["title"], "New Meeting");
    assert!(json["date"].is_string());
    assert!(json["status"].is_string());
    assert!(json["created_at"].is_string());
    assert!(json["updated_at"].is_string());
    
    // Metadata fields should be null or absent for new meetings
    // (they're populated via import process)
    assert!(json["participants"].is_null() || json.get("participants").is_none());
    assert!(json["location"].is_null() || json.get("location").is_none());
    assert!(json["organizer"].is_null() || json.get("organizer").is_none());
    assert!(json["metadata_source"].is_null() || json.get("metadata_source").is_none());
    assert!(json["file_metadata"].is_null() || json.get("file_metadata").is_none());
}

#[tokio::test]
async fn test_list_meetings_includes_metadata() {
    let (state, _temp_dir) = test_app_state();

    // Create meeting with metadata
    let mut meeting1 = Meeting::new("Meeting 1".to_string());
    meeting1.participants = Some(vec!["Alice".to_string()]);
    meeting1.location = Some("Room A".to_string());
    state.storage.create_meeting(&meeting1).unwrap();

    // Create meeting without metadata
    let meeting2 = Meeting::new("Meeting 2".to_string());
    state.storage.create_meeting(&meeting2).unwrap();

    let app = meeting_agent_server::build_router(state);

    let request = Request::builder()
        .uri("/meetings")
        .method("GET")
        .header("X-API-Key", "test-key")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert!(json["meetings"].is_array());
    let meetings = json["meetings"].as_array().unwrap();
    assert_eq!(meetings.len(), 2);

    // Find meeting with metadata
    let meeting_with_metadata = meetings
        .iter()
        .find(|m| m["title"] == "Meeting 1")
        .unwrap();
    
    assert_eq!(meeting_with_metadata["participants"][0], "Alice");
    assert_eq!(meeting_with_metadata["location"], "Room A");

    // Find meeting without metadata
    let meeting_without_metadata = meetings
        .iter()
        .find(|m| m["title"] == "Meeting 2")
        .unwrap();
    
    assert!(meeting_without_metadata["participants"].is_null() 
        || meeting_without_metadata.get("participants").is_none());
}

#[tokio::test]
async fn test_metadata_source_serialization() {
    let (state, _temp_dir) = test_app_state();

    // Test all metadata source variants
    let sources = vec![
        (MetadataSource::UserProvided, "userprovided"),
        (MetadataSource::CalendarBot, "calendarbot"),
        (MetadataSource::Filename, "filename"),
        (MetadataSource::FFprobe, "ffprobe"),
        (MetadataSource::Default, "default"),
    ];

    for (source, expected_json) in sources {
        let mut meeting = Meeting::new(format!("Test {}", expected_json));
        meeting.metadata_source = Some(source);
        let meeting_id = meeting.id.clone();
        state.storage.create_meeting(&meeting).unwrap();

        let app = meeting_agent_server::build_router(state.clone());

        let request = Request::builder()
            .uri(format!("/meetings/{}", meeting_id))
            .method("GET")
            .header("X-API-Key", "test-key")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["metadata_source"], expected_json);
    }
}

#[tokio::test]
async fn test_file_metadata_all_fields() {
    let (state, _temp_dir) = test_app_state();

    let mut meeting = Meeting::new("Test File Metadata".to_string());
    meeting.file_metadata = Some(FileMetadata {
        codec: Some("opus".to_string()),
        sample_rate: Some(48000),
        bit_rate: Some(64000),
        channels: Some(1),
        file_size_bytes: Some(1024768),
    });
    let meeting_id = meeting.id.clone();
    state.storage.create_meeting(&meeting).unwrap();

    let app = meeting_agent_server::build_router(state);

    let request = Request::builder()
        .uri(format!("/meetings/{}", meeting_id))
        .method("GET")
        .header("X-API-Key", "test-key")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    let file_meta = &json["file_metadata"];
    assert_eq!(file_meta["codec"], "opus");
    assert_eq!(file_meta["sample_rate"], 48000);
    assert_eq!(file_meta["bit_rate"], 64000);
    assert_eq!(file_meta["channels"], 1);
    assert_eq!(file_meta["file_size_bytes"], 1024768);
}
