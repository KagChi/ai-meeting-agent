use anyhow::Result;
use chrono::Utc;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use meeting_agent_core::{
    config::Config,
    fs,
    models::{MeetingStatus, Summary, SummaryStatus, SummaryTemplate},
    storage::MeetingStorage,
    summary::{SummarizeOptions, SummaryClient},
};

pub async fn run(id: String, template: String, language: Option<String>) -> Result<()> {
    let template = match template.as_str() {
        "full" => SummaryTemplate::Full,
        "key-points" | "keypoints" => SummaryTemplate::KeyPoints,
        "action-items" | "actionitems" => SummaryTemplate::ActionItems,
        "decisions" => SummaryTemplate::Decisions,
        other => {
            anyhow::bail!(
                "Unknown template: {}. Use: full, key-points, action-items, decisions",
                other
            )
        }
    };

    let storage = MeetingStorage::new();
    let full_id = storage.resolve_meeting_id(&id)?;
    let meeting = storage.get_meeting(&full_id)?;

    if meeting.status != MeetingStatus::Ready {
        anyhow::bail!(
            "Meeting is not ready (status: {:?}). Cannot summarize.",
            meeting.status
        );
    }

    let transcript_resp = storage.get_transcript(&full_id)?;

    let config_path = fs::config_path()?;
    let config = Config::load(&config_path)?;
    let client = SummaryClient::new(config.summary.clone())?;

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    pb.set_message("Generating summary...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let opts = SummarizeOptions {
        template: template.clone(),
        language: language.clone(),
    };
    let result = client.summarize(&transcript_resp, &opts).await?;

    pb.finish_with_message("Summary complete!".green().to_string());

    let now = Utc::now();
    let summary = Summary {
        id: uuid::Uuid::new_v4().to_string(),
        meeting_id: full_id.clone(),
        template: template.clone(),
        language,
        status: SummaryStatus::Completed,
        content: result.content.clone(),
        key_points: result.key_points.clone(),
        action_items: result.action_items.clone(),
        decisions: result.decisions.clone(),
        provider: config.summary.provider.clone(),
        model: config.summary.model.clone(),
        created_at: now,
        updated_at: now,
    };
    storage.save_summary(&full_id, &summary)?;

    println!(
        "\n{} {}",
        "Summary".bold().green(),
        format!("({:?})", template).yellow()
    );
    println!("{}", "═".repeat(60));
    println!("{}", result.content);

    if !result.key_points.is_empty() {
        println!("\n{}", "Key Points:".bold().cyan());
        for p in &result.key_points {
            println!("  {} {}", "•".cyan(), p);
        }
    }

    if !result.action_items.is_empty() {
        println!("\n{}", "Action Items:".bold().cyan());
        for a in &result.action_items {
            println!("  {} {}", "•".cyan(), a);
        }
    }

    if !result.decisions.is_empty() {
        println!("\n{}", "Decisions:".bold().cyan());
        for d in &result.decisions {
            println!("  {} {}", "•".cyan(), d);
        }
    }

    println!("\n{} Summary saved.", "✓".green().bold());

    Ok(())
}
