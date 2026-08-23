//! Encoding synthesized samples into something playable.
//!
//! Pure: takes a sample buffer, returns bytes.

use std::io::Cursor;

use hound::{SampleFormat, WavSpec, WavWriter};

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

/// Encode mono f32 samples to AAC in an M4A container, via macOS AudioToolbox.
///
/// Replaces a LAME MP3 encode. LAME is LGPL and was linked statically with
/// `lto` + `strip`, which makes the §6 relink right impossible to exercise
/// without shipping a parallel unstripped build purely as a compliance
/// artifact. AudioToolbox is part of the OS: nothing to bundle, no notice to
/// ship, no relink right to preserve. macOS has no MP3 *encoder* to offer, so
/// dropping LAME necessarily means changing container -- see ADR-0004.
///
/// Writes through a temporary file rather than encoding in memory. The
/// ExtAudioFile API is file-oriented, and the caller writes these bytes to disk
/// immediately afterwards anyway; an in-memory path would mean hand-rolling
/// AudioFile read/write callbacks for no benefit the reader can perceive.
#[cfg(target_os = "macos")]
pub(crate) fn encode_f32_to_m4a(samples: &[f32], sample_rate: u32) -> AppResult<Vec<u8>> {
    use std::ffi::c_void;
    use std::os::unix::ffi::OsStrExt;

    if samples.is_empty() {
        return Err(AppError::Tts("Supertonic returned empty M4A audio.".into()));
    }

    #[repr(C)]
    #[derive(Default)]
    struct AudioStreamBasicDescription {
        sample_rate: f64,
        format_id: u32,
        format_flags: u32,
        bytes_per_packet: u32,
        frames_per_packet: u32,
        bytes_per_frame: u32,
        channels_per_frame: u32,
        bits_per_channel: u32,
        reserved: u32,
    }

    #[repr(C)]
    struct AudioBuffer {
        number_channels: u32,
        data_byte_size: u32,
        data: *mut c_void,
    }

    #[repr(C)]
    struct AudioBufferList {
        number_buffers: u32,
        buffers: [AudioBuffer; 1],
    }

    // Four-character codes, spelled out so a typo is visible rather than a
    // hex constant nobody can check by eye.
    const fn fourcc(code: &[u8; 4]) -> u32 {
        ((code[0] as u32) << 24)
            | ((code[1] as u32) << 16)
            | ((code[2] as u32) << 8)
            | code[3] as u32
    }
    let format_aac = fourcc(b"aac ");
    let format_pcm = fourcc(b"lpcm");
    let file_type_m4a = fourcc(b"m4af");
    let prop_client_format = fourcc(b"cfmt");
    const ERASE_FILE: u32 = 1;
    // kLinearPCMFormatFlagIsFloat | kLinearPCMFormatFlagIsPacked
    const PCM_FLOAT_PACKED: u32 = 1 | 8;

    // One block per framework: two `#[link]` attributes on a single extern
    // block is a clippy `duplicated_attributes` error, and the linker needs
    // both names regardless.
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: *const c_void,
            buffer: *const u8,
            buf_len: isize,
            is_directory: bool,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "AudioToolbox", kind = "framework")]
    extern "C" {
        fn ExtAudioFileCreateWithURL(
            url: *const c_void,
            file_type: u32,
            stream_desc: *const AudioStreamBasicDescription,
            channel_layout: *const c_void,
            flags: u32,
            out_file: *mut *mut c_void,
        ) -> i32;
        fn ExtAudioFileSetProperty(
            file: *mut c_void,
            property_id: u32,
            data_size: u32,
            data: *const c_void,
        ) -> i32;
        fn ExtAudioFileWrite(file: *mut c_void, frames: u32, data: *const AudioBufferList) -> i32;
        fn ExtAudioFileDispose(file: *mut c_void) -> i32;
    }

    fn osstatus(status: i32, what: &str) -> AppResult<()> {
        if status == 0 {
            return Ok(());
        }
        // OSStatus is often a four-character code read as an integer, so show
        // both: `1718449215` means nothing, `'fmt?'` names the problem.
        let bytes = (status as u32).to_be_bytes();
        let code = if bytes.iter().all(|b| b.is_ascii_graphic()) {
            format!("'{}'", String::from_utf8_lossy(&bytes))
        } else {
            status.to_string()
        };
        Err(AppError::Tts(format!("{what} failed ({code})")))
    }

    let dir = std::env::temp_dir();
    let path = dir.join(format!("libretexts-reader-{}.m4a", uuid::Uuid::new_v4()));

    // Everything below owns raw handles, so each early return has to release
    // what it holds. Kept in one function with explicit cleanup rather than a
    // Drop wrapper: two handles, one linear path, and a guard type would be
    // more machinery than the thing it guards.
    let result = (|| -> AppResult<Vec<u8>> {
        let bytes = path.as_os_str().as_bytes();
        let url = unsafe {
            CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                bytes.as_ptr(),
                bytes.len() as isize,
                false,
            )
        };
        if url.is_null() {
            return Err(AppError::Tts("could not build a temp file URL".into()));
        }

        let output = AudioStreamBasicDescription {
            sample_rate: f64::from(sample_rate),
            format_id: format_aac,
            frames_per_packet: 1024,
            channels_per_frame: 1,
            ..Default::default()
        };
        let client = AudioStreamBasicDescription {
            sample_rate: f64::from(sample_rate),
            format_id: format_pcm,
            format_flags: PCM_FLOAT_PACKED,
            bytes_per_packet: 4,
            frames_per_packet: 1,
            bytes_per_frame: 4,
            channels_per_frame: 1,
            bits_per_channel: 32,
            reserved: 0,
        };

        let mut file: *mut c_void = std::ptr::null_mut();
        let status = unsafe {
            ExtAudioFileCreateWithURL(
                url,
                file_type_m4a,
                &output,
                std::ptr::null(),
                ERASE_FILE,
                &mut file,
            )
        };
        unsafe { CFRelease(url) };
        osstatus(status, "creating the M4A file")?;

        let encode = (|| -> AppResult<()> {
            osstatus(
                unsafe {
                    ExtAudioFileSetProperty(
                        file,
                        prop_client_format,
                        std::mem::size_of::<AudioStreamBasicDescription>() as u32,
                        (&client as *const AudioStreamBasicDescription).cast(),
                    )
                },
                "setting the client format",
            )?;

            let mut pcm = samples.to_vec();
            for sample in &mut pcm {
                *sample = sample.clamp(-1.0, 1.0);
            }
            let list = AudioBufferList {
                number_buffers: 1,
                buffers: [AudioBuffer {
                    number_channels: 1,
                    data_byte_size: (pcm.len() * std::mem::size_of::<f32>()) as u32,
                    data: pcm.as_mut_ptr().cast(),
                }],
            };
            osstatus(
                unsafe { ExtAudioFileWrite(file, pcm.len() as u32, &list) },
                "writing samples",
            )
        })();

        // Dispose flushes the encoder and finalizes the container, so it must
        // run even when the write above failed -- and its own status matters:
        // a failure here means the moov atom was never written and the file is
        // unplayable despite every earlier call returning noErr.
        let disposed = unsafe { ExtAudioFileDispose(file) };
        encode?;
        osstatus(disposed, "finalizing the M4A file")?;

        let encoded = std::fs::read(&path)
            .map_err(|error| AppError::Tts(format!("reading the encoded M4A: {error}")))?;
        if encoded.is_empty() {
            return Err(AppError::Tts("M4A encoder returned empty audio.".into()));
        }
        Ok(encoded)
    })();

    let _ = std::fs::remove_file(&path);
    result
}

/// Non-macOS stub.
///
/// The app ships for macOS only, but CI typechecks the crate on Linux, and a
/// `#[cfg]` on the function alone would leave every call site failing to
/// resolve there. Erroring at runtime keeps the Linux build honest about what
/// it can do rather than hiding the call sites behind cfgs of their own.
#[cfg(not(target_os = "macos"))]
pub(crate) fn encode_f32_to_m4a(_samples: &[f32], _sample_rate: u32) -> AppResult<Vec<u8>> {
    Err(AppError::Tts(
        "AAC export requires macOS (AudioToolbox).".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // AudioToolbox is a macOS framework; CI builds this crate on Linux, where
    // `encode_f32_to_m4a` is a compile-time stub. Gating the tests rather than
    // the function keeps the Linux gate honest -- it still typechecks every
    // call site.
    #[cfg(target_os = "macos")]
    #[test]
    fn writes_an_mp4_container_the_system_will_open() {
        let samples = tone(44_100);

        let m4a = encode_f32_to_m4a(&samples, SUPERTONIC_SAMPLE_RATE).unwrap();

        // Every ISO base-media file starts with a size field followed by the
        // `ftyp` box type. Checking the magic rather than the length is what
        // tells an actual container apart from a buffer of raw AAC frames,
        // which no player would open.
        assert_eq!(&m4a[4..8], b"ftyp", "not an MP4/M4A container");
        assert!(
            m4a.len() > 1_000,
            "a second of audio encoded to {} bytes",
            m4a.len()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn refuses_an_empty_sample_buffer() {
        let error = encode_f32_to_m4a(&[], SUPERTONIC_SAMPLE_RATE).unwrap_err();

        assert!(format!("{error}").contains("empty"), "unexpected: {error}");
    }

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

    // The compression assertion the LAME test used to make, kept against the
    // encoder that replaced it: an export nobody would want to download is a
    // regression whichever codec produced it.
    #[cfg(target_os = "macos")]
    #[test]
    fn encodes_smaller_than_the_raw_samples() {
        let samples = tone(44_100);

        let m4a = encode_f32_to_m4a(&samples, SUPERTONIC_SAMPLE_RATE).unwrap();

        assert!(
            m4a.len() < samples.len() * 2,
            "m4a was {} bytes for {} samples",
            m4a.len(),
            samples.len()
        );
    }

    #[test]
    fn refuses_to_encode_nothing() {
        // An empty buffer means synthesis failed upstream; writing a zero-length
        // file would hide that behind a silent chapter.
        assert!(encode_f32_to_wav(&[], SUPERTONIC_SAMPLE_RATE).is_err());
    }
}
