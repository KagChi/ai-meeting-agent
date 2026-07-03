use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::diarize::error::{DiarizeError, Result};

const TARGET_SAMPLE_RATE: u32 = 16000;

pub fn decode_audio_to_f32_mono_16k(bytes: &[u8]) -> Result<Vec<f32>> {
    log::debug!("[audio] decoding {} bytes", bytes.len());

    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let prober = symphonia::default::get_probe();
    let hint = symphonia::core::probe::Hint::new();

    let mut format = prober
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| DiarizeError::AudioDecodeError(format!("probe failed: {e}")))?
        .format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| DiarizeError::AudioDecodeError("no decodable track".into()))?
        .clone();

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| DiarizeError::AudioDecodeError(format!("decoder init: {e}")))?;

    let spec = track
        .codec_params
        .channels
        .map(|ch| symphonia::core::audio::SignalSpec::new(src_rate_from(&track), ch))
        .ok_or_else(|| DiarizeError::AudioDecodeError("unknown channel layout".into()))?;

    let src_rate = src_rate_from(&track);
    if src_rate == 0 {
        return Err(DiarizeError::AudioDecodeError("zero sample rate".into()));
    }
    let chans = spec.channels.count();

    log::debug!(
        "[audio] detected: sample_rate={}Hz, channels={}, codec={:?}",
        src_rate,
        chans,
        track.codec_params.codec
    );

    let mut interleaved: Vec<f32> = Vec::new();
    let mut packet_count = 0;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::ResetRequired) => continue,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(e) => {
                log::debug!("[audio] decode loop end: {e}");
                break;
            }
        };

        let decoded = decoder
            .decode(&packet)
            .map_err(|e| DiarizeError::AudioDecodeError(format!("decode frame: {e}")))?;

        let frames = decoded.frames();
        let mut buf = SampleBuffer::<f32>::new(frames as u64, spec);
        buf.copy_interleaved_ref(decoded);

        let raw = buf.samples();
        // Downmix interleaved multi-channel → mono.
        if chans == 1 {
            interleaved.extend_from_slice(raw);
        } else {
            let mut i = 0;
            while i + chans <= raw.len() {
                let chunk = &raw[i..i + chans];
                let mean: f32 = chunk.iter().sum::<f32>() / chans as f32;
                interleaved.push(mean);
                i += chans;
            }
        }
        packet_count += 1;
    }

    log::debug!(
        "[audio] decoded {} packets → {} mono samples",
        packet_count,
        interleaved.len()
    );

    if src_rate != TARGET_SAMPLE_RATE {
        log::debug!(
            "[audio] resampling {}Hz → {}Hz",
            src_rate,
            TARGET_SAMPLE_RATE
        );
        interleaved = resample_linear(&interleaved, src_rate, TARGET_SAMPLE_RATE);
        log::debug!("[audio] resampled to {} samples", interleaved.len());
    }

    log::debug!(
        "[audio] decode complete: {} samples ({:.2}s at {}Hz)",
        interleaved.len(),
        interleaved.len() as f64 / TARGET_SAMPLE_RATE as f64,
        TARGET_SAMPLE_RATE
    );

    Ok(interleaved)
}

fn src_rate_from(track: &symphonia::core::formats::Track) -> u32 {
    track.codec_params.sample_rate.unwrap_or(0)
}

fn resample_linear(samples: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == dst_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = dst_rate as f64 / src_rate as f64;
    let out_len = ((samples.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 / ratio;
        let idx0 = src_pos.floor() as usize;
        let idx1 = (idx0 + 1).min(samples.len() - 1);
        let frac = src_pos - idx0 as f64;
        let v = samples[idx0] as f64 * (1.0 - frac) + samples[idx1] as f64 * frac;
        out.push(v as f32);
    }
    out
}
