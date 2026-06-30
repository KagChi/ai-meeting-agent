use std::path::PathBuf;

use meeting_agent_diarize::{
    audio::decode_audio_to_f32_mono_16k, merge, validate_whisper_segments, DiarizeConfig,
    DiarizeResponse, SpeakerDiarizer, WhisperTranscript,
};

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: diarize_file <audio.mp3|wav> <transcript.json>");
        std::process::exit(2);
    }
    let audio_path = PathBuf::from(&args[1]);
    let transcript_path = PathBuf::from(&args[2]);

    let audio_bytes = std::fs::read(&audio_path)?;
    let transcript_bytes = std::fs::read(&transcript_path)?;

    let cfg = DiarizeConfig::from_env().map_err(|e| anyhow::anyhow!(e))?;
    let diarizer = SpeakerDiarizer::new(&cfg).map_err(|e| anyhow::anyhow!(e))?;

    let transcript: WhisperTranscript = serde_json::from_slice(&transcript_bytes)?;
    let segments = validate_whisper_segments(transcript.segments);

    let samples = decode_audio_to_f32_mono_16k(&audio_bytes).map_err(|e| anyhow::anyhow!(e))?;

    let (num_speakers, speaker_segments) =
        diarizer.process(&samples).map_err(|e| anyhow::anyhow!(e))?;

    let cleaned = merge(segments, speaker_segments);
    let resp = DiarizeResponse {
        num_speakers,
        segments: cleaned,
    };

    println!("{}", serde_json::to_string_pretty(&resp)?);
    Ok(())
}
