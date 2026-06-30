use anyhow::Result;
use colored::Colorize;
use meeting_agent_core::{storage::MeetingStorage, transcription::TranscriptionResponse};

pub async fn run(id: String, format: String, output: Option<String>) -> Result<()> {
    let storage = MeetingStorage::new();
    let full_id = storage.resolve_meeting_id(&id)?;
    let resp = storage.get_transcript(&full_id)?;

    let content = match format.as_str() {
        "srt" => to_srt(&resp),
        "vtt" => to_vtt(&resp),
        "text" => resp.text,
        "json" => serde_json::to_string_pretty(&resp)?,
        other => anyhow::bail!("Unknown format: {}. Use: srt, vtt, text, json", other),
    };

    match output {
        Some(path) => {
            std::fs::write(&path, &content)?;
            println!("{} Exported to {}", "✓".green().bold(), path);
        }
        None => print!("{}", content),
    }

    Ok(())
}

fn format_timestamp(seconds: f64, sep: &str) -> String {
    let h = (seconds / 3600.0) as u32;
    let m = ((seconds % 3600.0) / 60.0) as u32;
    let s = (seconds % 60.0) as u32;
    let ms = ((seconds % 1.0) * 1000.0) as u32;
    format!("{:02}{}{:02}{}{:02}{}{:03}", h, sep, m, sep, s, sep, ms)
}

fn to_srt(resp: &TranscriptionResponse) -> String {
    let segments = resp.segments.as_deref().unwrap_or(&[]);
    segments
        .iter()
        .enumerate()
        .map(|(i, s)| {
            format!(
                "{}\n{} --> {}\n{}\n\n",
                i + 1,
                format_timestamp(s.start, ":"),
                format_timestamp(s.end, ":"),
                s.text.trim()
            )
        })
        .collect()
}

fn to_vtt(resp: &TranscriptionResponse) -> String {
    let mut out = "WEBVTT\n\n".to_string();
    let segments = resp.segments.as_deref().unwrap_or(&[]);
    for s in segments {
        out.push_str(&format!(
            "{} --> {}\n{}\n\n",
            format_timestamp(s.start, "."),
            format_timestamp(s.end, "."),
            s.text.trim()
        ));
    }
    out
}
