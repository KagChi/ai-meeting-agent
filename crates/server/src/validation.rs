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
const MAX_LOCATION_LEN: usize = 200;
const MAX_ORGANIZER_LEN: usize = 200;
const MAX_SPEAKER_MAPPINGS: usize = 50;
const MAX_SPEAKER_LABEL_LEN: usize = 100;

/// Validate UpdateMeetingRequest has at least one field
pub fn validate_update_request(
    title: &Option<String>,
    date: &Option<chrono::DateTime<chrono::Utc>>,
    participants: &Option<Vec<String>>,
    location: &Option<String>,
    organizer: &Option<String>,
) -> Result<(), ApiError> {
    if title.is_none()
        && date.is_none()
        && participants.is_none()
        && location.is_none()
        && organizer.is_none()
    {
        return Err(ApiError::BadRequest(
            "At least one field (title, date, participants, location, or organizer) must be provided"
                .to_string(),
        ));
    }
    if let Some(t) = title {
        validate_meeting_title(t)?;
    }
    if let Some(list) = participants {
        validate_participants(list)?;
    }
    if let Some(loc) = location {
        validate_optional_text_field("Location", loc, MAX_LOCATION_LEN)?;
    }
    if let Some(org) = organizer {
        validate_optional_text_field("Organizer", org, MAX_ORGANIZER_LEN)?;
    }
    Ok(())
}

/// Optional free-text field: empty string allowed (clears field); non-empty must be ≤ max_len.
fn validate_optional_text_field(label: &str, value: &str, max_len: usize) -> Result<(), ApiError> {
    let trimmed = value.trim();
    if !trimmed.is_empty() && trimmed.len() > max_len {
        return Err(ApiError::BadRequest(format!(
            "{label} must be {max_len} characters or less"
        )));
    }
    Ok(())
}

/// Validate bulk speaker rename mapping.
pub fn validate_speaker_mapping(
    mapping: &std::collections::HashMap<String, String>,
) -> Result<(), ApiError> {
    if mapping.is_empty() {
        return Err(ApiError::BadRequest(
            "mapping must contain at least one entry".to_string(),
        ));
    }
    if mapping.len() > MAX_SPEAKER_MAPPINGS {
        return Err(ApiError::BadRequest(format!(
            "At most {MAX_SPEAKER_MAPPINGS} speaker renames allowed"
        )));
    }
    for (old, new) in mapping {
        let old_t = old.trim();
        let new_t = new.trim();
        if old_t.is_empty() {
            return Err(ApiError::BadRequest(
                "Speaker labels to rename cannot be empty".to_string(),
            ));
        }
        if new_t.is_empty() {
            return Err(ApiError::BadRequest(
                "New speaker names cannot be empty".to_string(),
            ));
        }
        if old_t.len() > MAX_SPEAKER_LABEL_LEN || new_t.len() > MAX_SPEAKER_LABEL_LEN {
            return Err(ApiError::BadRequest(format!(
                "Speaker labels must be {MAX_SPEAKER_LABEL_LEN} characters or less"
            )));
        }
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

const MAX_PERSON_NAME_LEN: usize = 100;
const MAX_PERSON_ALIASES: usize = 20;
const MAX_ALIAS_LEN: usize = 100;

/// Validate voice-bank person display name.
pub fn validate_person_name(name: &str) -> Result<(), ApiError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest(
            "Person name cannot be empty".to_string(),
        ));
    }
    if trimmed.len() > MAX_PERSON_NAME_LEN {
        return Err(ApiError::BadRequest(format!(
            "Person name must be {MAX_PERSON_NAME_LEN} characters or less"
        )));
    }
    Ok(())
}

/// Validate optional aliases list for a person.
pub fn validate_person_aliases(aliases: &[String]) -> Result<(), ApiError> {
    if aliases.len() > MAX_PERSON_ALIASES {
        return Err(ApiError::BadRequest(format!(
            "At most {MAX_PERSON_ALIASES} aliases allowed"
        )));
    }
    for alias in aliases {
        let trimmed = alias.trim();
        if trimmed.is_empty() {
            return Err(ApiError::BadRequest(
                "Aliases cannot be empty".to_string(),
            ));
        }
        if trimmed.len() > MAX_ALIAS_LEN {
            return Err(ApiError::BadRequest(format!(
                "Alias must be {MAX_ALIAS_LEN} characters or less"
            )));
        }
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
        assert!(validate_update_request(&title, &date, &None, &None, &None).is_ok());
    }

    #[test]
    fn test_validate_update_request_title_only() {
        let title = Some("Title".to_string());
        assert!(validate_update_request(&title, &None, &None, &None, &None).is_ok());
    }

    #[test]
    fn test_validate_update_request_date_only() {
        let date = Some(chrono::Utc::now());
        assert!(validate_update_request(&None, &date, &None, &None, &None).is_ok());
    }

    #[test]
    fn test_validate_update_request_participants_only() {
        let participants = Some(vec!["Alice".to_string(), "Bob".to_string()]);
        assert!(validate_update_request(&None, &None, &participants, &None, &None).is_ok());
    }

    #[test]
    fn test_validate_update_request_location_only() {
        let location = Some("Room A".to_string());
        assert!(validate_update_request(&None, &None, &None, &location, &None).is_ok());
    }

    #[test]
    fn test_validate_update_request_organizer_only() {
        let organizer = Some("Alice".to_string());
        assert!(validate_update_request(&None, &None, &None, &None, &organizer).is_ok());
    }

    #[test]
    fn test_validate_update_request_empty() {
        assert!(validate_update_request(&None, &None, &None, &None, &None).is_err());
    }

    #[test]
    fn test_validate_update_request_invalid_title() {
        let title = Some("".to_string());
        assert!(validate_update_request(&title, &None, &None, &None, &None).is_err());
    }

    #[test]
    fn test_validate_speaker_mapping_ok() {
        let mut m = std::collections::HashMap::new();
        m.insert("SPEAKER_00".to_string(), "Alice".to_string());
        assert!(validate_speaker_mapping(&m).is_ok());
    }

    #[test]
    fn test_validate_speaker_mapping_empty() {
        let m = std::collections::HashMap::new();
        assert!(validate_speaker_mapping(&m).is_err());
    }

    #[test]
    fn test_validate_speaker_mapping_empty_new() {
        let mut m = std::collections::HashMap::new();
        m.insert("SPEAKER_00".to_string(), "  ".to_string());
        assert!(validate_speaker_mapping(&m).is_err());
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
