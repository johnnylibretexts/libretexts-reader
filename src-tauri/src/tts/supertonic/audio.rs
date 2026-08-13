//! Encoding synthesized samples into something playable.
//!
//! Pure: takes a sample buffer, returns bytes.

use std::io::Cursor;

use hound::{SampleFormat, WavSpec, WavWriter};
use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, MonoPcm, Quality};

use crate::error::{AppError, AppResult};

pub(crate) const SUPERTONIC_SAMPLE_RATE: u32 = 44_100;

pub(crate) fn encode_f32_to_wav(samples: &[f32], sample_rate: u32) -> AppResult<Vec<u8>> {
    if samples.is_empty() {
        return Err(AppError::Tts("Supertonic returned empty WAV audio.".into()));
    }

    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer =
            WavWriter::new(&mut cursor, spec).map_err(|error| AppError::Tts(error.to_string()))?;
        for &sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            writer
                .write_sample((clamped * 32767.0) as i16)
                .map_err(|error| AppError::Tts(error.to_string()))?;
        }
        writer
            .finalize()
            .map_err(|error| AppError::Tts(error.to_string()))?;
    }
    Ok(cursor.into_inner())
}

pub(crate) fn encode_f32_to_mp3(samples: &[f32], sample_rate: u32) -> AppResult<Vec<u8>> {
    if samples.is_empty() {
        return Err(AppError::Tts("Supertonic returned empty MP3 audio.".into()));
    }

    let pcm = samples
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect::<Vec<_>>();
    let mut encoder = Builder::new()
        .ok_or_else(|| AppError::Tts("could not initialize MP3 encoder".into()))?
        .with_num_channels(1)
        .map_err(|error| AppError::Tts(error.to_string()))?
        .with_sample_rate(sample_rate)
        .map_err(|error| AppError::Tts(error.to_string()))?
        .with_brate(Bitrate::Kbps128)
        .map_err(|error| AppError::Tts(error.to_string()))?
        .with_quality(Quality::Best)
        .map_err(|error| AppError::Tts(error.to_string()))?
        .build()
        .map_err(|error| AppError::Tts(error.to_string()))?;
    let mut output = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(pcm.len()));
    encoder
        .encode_to_vec(MonoPcm(&pcm), &mut output)
        .map_err(|error| AppError::Tts(error.to_string()))?;
    output.reserve(7200);
    encoder
        .flush_to_vec::<FlushNoGap>(&mut output)
        .map_err(|error| AppError::Tts(error.to_string()))?;

    if output.is_empty() {
        return Err(AppError::Tts("MP3 encoder returned empty audio.".into()));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|i| (i as f32 / 100.0).sin() * 0.5)
            .collect()
    }

    #[test]
    fn writes_a_riff_header_and_every_sample() {
        let samples = tone(1_000);

        let wav = encode_f32_to_wav(&samples, SUPERTONIC_SAMPLE_RATE).unwrap();

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        // 16-bit mono: two bytes per sample, plus a 44-byte header.
        assert_eq!(wav.len(), 44 + samples.len() * 2);
    }

    #[test]
    fn encodes_mp3_smaller_than_the_raw_samples() {
        let samples = tone(44_100);

        let mp3 = encode_f32_to_mp3(&samples, SUPERTONIC_SAMPLE_RATE).unwrap();

        assert!(!mp3.is_empty());
        assert!(mp3.len() < samples.len() * 2, "mp3 was {} bytes", mp3.len());
    }

    #[test]
    fn refuses_to_encode_nothing() {
        // An empty buffer means synthesis failed upstream; writing a zero-length
        // file would hide that behind a silent chapter.
        assert!(encode_f32_to_wav(&[], SUPERTONIC_SAMPLE_RATE).is_err());
        assert!(encode_f32_to_mp3(&[], SUPERTONIC_SAMPLE_RATE).is_err());
    }
}
