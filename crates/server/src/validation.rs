use crate::error::ApiError;

/// Validate meeting title
/// - Must be non-empty
/// - Max 200 characters
pub fn validate_meeting_title(title: &str) -> Result<(), ApiError> {
    if title.trim().is_empty() {
        return Err(ApiError::BadRequest("Title cannot be empty".to_string()));
    }
    if title.len() > 200 {
        return Err(ApiError::BadRequest(
            "Title must be 200 characters or less".to_string(),
        ));
    }
    Ok(())
}

/// Validate UUID format
pub fn validate_uuid(id: &str) -> Result<(), ApiError> {
    uuid::Uuid::parse_str(id)
        .map_err(|_| ApiError::BadRequest(format!("Invalid UUID format: {}", id)))?;
    Ok(())
}

const MAX_PARTICIPANTS: usize = 50;
const MAX_PARTICIPANT_NAME_LEN: usize = 100;

/// Validate UpdateMeetingRequest has at least one field
pub fn validate_update_request(
    title: &Option<String>,
    date: &Option<chrono::DateTime<chrono::Utc>>,
    participants: &Option<Vec<String>>,
) -> Result<(), ApiError> {
    if title.is_none() && date.is_none() && participants.is_none() {
        return Err(ApiError::BadRequest(
            "At least one field (title, date, or participants) must be provided".to_string(),
        ));
    }
    if let Some(t) = title {
        validate_meeting_title(t)?;
    }
    if let Some(list) = participants {
        validate_participants(list)?;
    }
    Ok(())
}

/// Validate participants list (names trimmed; empty strings rejected)
pub fn validate_participants(participants: &[String]) -> Result<(), ApiError> {
    if participants.len() > MAX_PARTICIPANTS {
        return Err(ApiError::BadRequest(format!(
            "At most {MAX_PARTICIPANTS} participants allowed"
        )));
    }
    for name in participants {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ApiError::BadRequest(
                "Participant names cannot be empty".to_string(),
            ));
        }
        if trimmed.len() > MAX_PARTICIPANT_NAME_LEN {
            return Err(ApiError::BadRequest(format!(
                "Participant name must be {MAX_PARTICIPANT_NAME_LEN} characters or less"
            )));
        }
    }
    Ok(())
}

/// Validate freeform summary content for PUT
pub fn validate_summary_content(content: &str) -> Result<(), ApiError> {
    if content.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "Summary content cannot be empty".to_string(),
        ));
    }
    if content.len() > 500_000 {
        return Err(ApiError::BadRequest(
            "Summary content must be 500000 characters or less".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_meeting_title_valid() {
        assert!(validate_meeting_title("Valid Title").is_ok());
        assert!(validate_meeting_title("A").is_ok());
        assert!(validate_meeting_title("  Valid Title  ").is_ok());
    }

    #[test]
    fn test_validate_meeting_title_empty() {
        assert!(validate_meeting_title("").is_err());
        assert!(validate_meeting_title("   ").is_err());
    }

    #[test]
    fn test_validate_meeting_title_too_long() {
        let long_title = "a".repeat(201);
        assert!(validate_meeting_title(&long_title).is_err());
    }

    #[test]
    fn test_validate_meeting_title_max_length() {
        let max_title = "a".repeat(200);
        assert!(validate_meeting_title(&max_title).is_ok());
    }

    #[test]
    fn test_validate_uuid_valid() {
        assert!(validate_uuid("550e8400-e29b-41d4-a716-446655440000").is_ok());
    }

    #[test]
    fn test_validate_uuid_invalid() {
        assert!(validate_uuid("not-a-uuid").is_err());
        assert!(validate_uuid("").is_err());
        assert!(validate_uuid("123").is_err());
    }

    #[test]
    fn test_validate_update_request_both_fields() {
        let title = Some("Title".to_string());
        let date = Some(chrono::Utc::now());
        assert!(validate_update_request(&title, &date, &None).is_ok());
    }

    #[test]
    fn test_validate_update_request_title_only() {
        let title = Some("Title".to_string());
        assert!(validate_update_request(&title, &None, &None).is_ok());
    }

    #[test]
    fn test_validate_update_request_date_only() {
        let date = Some(chrono::Utc::now());
        assert!(validate_update_request(&None, &date, &None).is_ok());
    }

    #[test]
    fn test_validate_update_request_participants_only() {
        let participants = Some(vec!["Alice".to_string(), "Bob".to_string()]);
        assert!(validate_update_request(&None, &None, &participants).is_ok());
    }

    #[test]
    fn test_validate_update_request_empty() {
        assert!(validate_update_request(&None, &None, &None).is_err());
    }

    #[test]
    fn test_validate_update_request_invalid_title() {
        let title = Some("".to_string());
        assert!(validate_update_request(&title, &None, &None).is_err());
    }

    #[test]
    fn test_validate_participants_empty_name() {
        assert!(validate_participants(&["".to_string()]).is_err());
        assert!(validate_participants(&["  ".to_string()]).is_err());
    }

    #[test]
    fn test_validate_summary_content() {
        assert!(validate_summary_content("hello").is_ok());
        assert!(validate_summary_content("").is_err());
        assert!(validate_summary_content("   ").is_err());
    }
}
