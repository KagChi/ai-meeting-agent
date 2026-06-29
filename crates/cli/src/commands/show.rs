use anyhow::Result;
use colored::Colorize;
use meeting_agent_core::{models::MeetingStatus, MeetingStorage};

pub async fn run(id: String) -> Result<()> {
    let storage = MeetingStorage;

    let full_id = storage.resolve_meeting_id(&id)?;
    let meeting = storage.get_meeting(&full_id)?;

    println!("{}", "Meeting Details".bold().green());
    println!("{}", "─".repeat(50));
    println!("{} {}", "ID:".bold(), meeting.id.cyan());
    println!("{} {}", "Title:".bold(), meeting.title);
    println!(
        "{} {}",
        "Date:".bold(),
        meeting.date.format("%Y-%m-%d %H:%M:%S UTC")
    );

    if let Some(d) = meeting.duration_seconds {
        println!(
            "{} {:02}:{:02}:{:02}",
            "Duration:".bold(),
            d / 3600,
            (d % 3600) / 60,
            d % 60
        );
    }

    let status_str = match meeting.status {
        MeetingStatus::Ready => "Ready".green().to_string(),
        MeetingStatus::Importing => "Importing".yellow().to_string(),
        MeetingStatus::Failed => "Failed".red().to_string(),
    };
    println!("{} {}", "Status:".bold(), status_str);

    if let Some(t) = &meeting.transcription {
        println!("{} {} ({})", "Transcription:".bold(), t.provider, t.model);
    }

    println!(
        "\n{} {}",
        "Created:".bold(),
        meeting.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!(
        "{} {}",
        "Updated:".bold(),
        meeting.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
    );

    if let Ok(resp) = storage.get_transcript(&full_id) {
        println!("\n{}", "Transcript Preview:".bold().green());
        println!("{}", "─".repeat(50));
        let preview = if resp.text.len() > 500 {
            format!("{}...", &resp.text[..500])
        } else {
            resp.text.clone()
        };
        println!("{}", preview);
    }

    if let Ok(summaries) = storage.list_summaries(&full_id) {
        if !summaries.is_empty() {
            println!("\n{}", "Summaries:".bold().green());
            for s in &summaries {
                println!(
                    "  {} {} ({})",
                    "•".cyan(),
                    format!("{:?}", s.template).yellow(),
                    s.created_at.format("%Y-%m-%d %H:%M")
                );
            }
        }
    }

    Ok(())
}
