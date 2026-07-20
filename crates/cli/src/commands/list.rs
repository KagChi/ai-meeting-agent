use anyhow::Result;
use colored::Colorize;
use comfy_table::{presets::UTF8_FULL, Cell, Color, Table};
use meeting_agent_core::{models::MeetingStatus, MeetingStorage};

pub async fn run() -> Result<()> {
    let storage = MeetingStorage::new().await?;
    let meetings = storage.list_meetings().await?;

    if meetings.is_empty() {
        println!("{}", "No meetings found.".yellow());
        println!("Import one with: {}", "meeting-agent import <file>".cyan());
        return Ok(());
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        Cell::new("ID").fg(Color::Cyan),
        Cell::new("Title").fg(Color::Cyan),
        Cell::new("Date").fg(Color::Cyan),
        Cell::new("Duration").fg(Color::Cyan),
        Cell::new("Status").fg(Color::Cyan),
    ]);

    for m in &meetings {
        let duration = m
            .duration_seconds
            .map(|d| format!("{:02}:{:02}:{:02}", d / 3600, (d % 3600) / 60, d % 60))
            .unwrap_or_else(|| "—".to_string());
        let status = match m.status {
            MeetingStatus::Ready => "Ready".green().to_string(),
            MeetingStatus::Importing => "Importing".yellow().to_string(),
            MeetingStatus::Failed => "Failed".red().to_string(),
        };
        table.add_row(vec![
            Cell::new(&m.id[..8]),
            Cell::new(&m.title),
            Cell::new(m.date.format("%Y-%m-%d %H:%M").to_string()),
            Cell::new(duration),
            Cell::new(status),
        ]);
    }

    println!("{table}");
    println!("\n{} meeting(s) total", meetings.len().to_string().bold());

    Ok(())
}
