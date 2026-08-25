//! Running the model.
//!
//! `Synthesizer` is the seam. Production loads ONNX sessions; tests use
//! `FakeSynthesizer` and never touch the 394 MB model bundle. Two adapters, so
//! the seam is real rather than a hypothetical one.
//!
//! On locking: `ort`'s `Session::run` takes `&mut self`, so inference cannot
//! run concurrently through one loaded model however the locks are arranged.
//! What the split below does buy is that the registry lock is never held across
//! a model load or an inference — so a panic inside inference can no longer
//! poison the one lock that every future synthesis has to pass through.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ndarray::{Array, Array3};
use once_cell::sync::Lazy;
use ort::{session::Session, value::Value as OrtValue};
use rand_distr::{Distribution, Normal};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::tts::supertonic::audio::SUPERTONIC_SAMPLE_RATE;
use crate::tts::supertonic::chunk::chunk_text_for_language;
use crate::tts::supertonic::model::{supertonic_model_dir, supertonic_model_status};
use crate::tts::supertonic::text::{length_to_mask, UnicodeProcessor};
use crate::tts::supertonic::{SUPERTONIC_SILENCE_SECONDS, SUPERTONIC_TOTAL_STEPS};

/// Anything that can turn text into samples.
pub(crate) trait Synthesizer: Send {
    fn synthesize(
        &mut self,
        text: &str,
        language: &str,
        voice_style: &str,
        speed: f32,
    ) -> AppResult<Vec<f32>>;
}

struct CachedEngine {
    directory: PathBuf,
    synthesizer: Arc<Mutex<dyn Synthesizer>>,
}

static ENGINE: Lazy<Mutex<Option<CachedEngine>>> = Lazy::new(|| Mutex::new(None));

/// Synthesize one piece of text. Blocking; callers run it on a blocking thread.
pub(crate) fn synthesize_samples_blocking(
    text: &str,
    voice_style: &str,
    language: &str,
    speed: f32,
) -> AppResult<Vec<f32>> {
    let synthesizer = active_synthesizer()?;

    // A panic during inference poisons only this engine's lock, and the state
    // behind it is a set of ONNX sessions that carry nothing between runs — so
    // recovering the value is safe, where dying forever was not.
    let mut guard = synthesizer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let samples = guard.synthesize(text, language, voice_style, speed)?;

    if samples.is_empty() {
        return Err(AppError::Tts("Supertonic returned empty audio.".into()));
    }
    Ok(samples)
}

fn active_synthesizer() -> AppResult<Arc<Mutex<dyn Synthesizer>>> {
    if fake_audio_enabled() {
        return Ok(Arc::new(Mutex::new(FakeSynthesizer)));
    }

    ensure_model_ready()?;
    let directory = supertonic_model_dir()?;

    {
        let registry = lock_registry();
        if let Some(cached) = registry.as_ref() {
            if cached.directory == directory {
                return Ok(Arc::clone(&cached.synthesizer));
            }
        }
    }

    // Loaded outside the registry lock: this reads ~394 MB from disk, and
    // holding the lock through it would block every other request. Two callers
    // racing here each build one; the later store wins and the loser is dropped.
    let synthesizer: Arc<Mutex<dyn Synthesizer>> =
        Arc::new(Mutex::new(OnnxSynthesizer::load(&directory)?));

    let mut registry = lock_registry();
    *registry = Some(CachedEngine {
        directory,
        synthesizer: Arc::clone(&synthesizer),
    });
    Ok(synthesizer)
}

/// The registry lock guards a pointer swap and nothing that can panic, so
/// recovering a poisoned one cannot expose a half-written value.
fn lock_registry() -> std::sync::MutexGuard<'static, Option<CachedEngine>> {
    ENGINE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn ensure_model_ready() -> AppResult<()> {
    let status = supertonic_model_status()?;
    if !status.downloaded {
        return Err(AppError::Model(
            "Supertonic model is not downloaded. Download it in Settings.".into(),
        ));
    }
    Ok(())
}

struct OnnxSynthesizer {
    directory: PathBuf,
    tts: TextToSpeech,
}

impl OnnxSynthesizer {
    fn load(directory: &Path) -> AppResult<Self> {
        Ok(Self {
            directory: directory.to_path_buf(),
            tts: load_text_to_speech(&directory.join("onnx"))?,
        })
    }
}

impl Synthesizer for OnnxSynthesizer {
    fn synthesize(
        &mut self,
        text: &str,
        language: &str,
        voice_style: &str,
        speed: f32,
    ) -> AppResult<Vec<f32>> {
        let style_path = self
            .directory
            .join("voice_styles")
            .join(format!("{voice_style}.json"));
        let style = load_voice_style(&[style_path])?;
        let (samples, _duration) = self.tts.call(
            text,
            language,
            &style,
            SUPERTONIC_TOTAL_STEPS,
            speed,
            SUPERTONIC_SILENCE_SECONDS,
        )?;
        Ok(samples)
    }
}

/// A tone instead of speech. Previously an env-var branch buried inside the
/// synthesis path; promoting it to an adapter is what makes the seam real.
pub(crate) struct FakeSynthesizer;

impl Synthesizer for FakeSynthesizer {
    fn synthesize(
        &mut self,
        text: &str,
        _language: &str,
        _voice_style: &str,
        _speed: f32,
    ) -> AppResult<Vec<f32>> {
        Ok(fake_samples(text))
    }
}

fn fake_audio_enabled() -> bool {
    std::env::var_os("LIBRETEXTS_READER_SUPERTONIC_FAKE_AUDIO").is_some()
}

fn fake_samples(text: &str) -> Vec<f32> {
    let seconds = (0.15 + (text.chars().count() as f32 / 80.0)).clamp(0.2, 1.5);
    let sample_count = (SUPERTONIC_SAMPLE_RATE as f32 * seconds) as usize;
    (0..sample_count)
        .map(|index| {
            let phase =
                (index as f32 / SUPERTONIC_SAMPLE_RATE as f32) * 440.0 * std::f32::consts::TAU;
            phase.sin() * 0.12
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    ae: AEConfig,
    ttl: TTLConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AEConfig {
    sample_rate: i32,
    base_chunk_size: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TTLConfig {
    chunk_compress_factor: i32,
    latent_dim: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VoiceStyleData {
    style_ttl: StyleComponent,
    style_dp: StyleComponent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StyleComponent {
    data: Vec<Vec<Vec<f32>>>,
    dims: Vec<usize>,
    #[serde(rename = "type")]
    dtype: String,
}

struct Style {
    ttl: Array3<f32>,
    dp: Array3<f32>,
}

struct TextToSpeech {
    cfgs: Config,
    text_processor: UnicodeProcessor,
    dp_ort: Session,
    text_enc_ort: Session,
    vector_est_ort: Session,
    vocoder_ort: Session,
    sample_rate: i32,
}

impl TextToSpeech {
    fn new(
        cfgs: Config,
        text_processor: UnicodeProcessor,
        dp_ort: Session,
        text_enc_ort: Session,
        vector_est_ort: Session,
        vocoder_ort: Session,
    ) -> Self {
        let sample_rate = cfgs.ae.sample_rate;
        Self {
            cfgs,
            text_processor,
            dp_ort,
            text_enc_ort,
            vector_est_ort,
            vocoder_ort,
            sample_rate,
        }
    }

    fn infer(
        &mut self,
        text_list: &[String],
        lang_list: &[String],
        style: &Style,
        total_step: usize,
        speed: f32,
    ) -> AppResult<(Vec<f32>, Vec<f32>)> {
        let bsz = text_list.len();
        if bsz == 0 {
            return Err(AppError::InvalidInput("text is required".into()));
        }

        let (text_ids, text_mask) = self.text_processor.call(text_list, lang_list)?;
        let text_ids_array = {
            let text_ids_shape = (bsz, text_ids[0].len());
            let mut flat = Vec::new();
            for row in &text_ids {
                flat.extend_from_slice(row);
            }
            Array::from_shape_vec(text_ids_shape, flat)
                .map_err(|error| AppError::Tts(error.to_string()))?
        };

        let text_ids_value = OrtValue::from_array(text_ids_array)
            .map_err(|error| AppError::Tts(error.to_string()))?;
        let text_mask_value = OrtValue::from_array(text_mask.clone())
            .map_err(|error| AppError::Tts(error.to_string()))?;
        let style_dp_value = OrtValue::from_array(style.dp.clone())
            .map_err(|error| AppError::Tts(error.to_string()))?;

        let dp_outputs = self
            .dp_ort
            .run(ort::inputs! {
                "text_ids" => &text_ids_value,
                "style_dp" => &style_dp_value,
                "text_mask" => &text_mask_value
            })
            .map_err(|error| AppError::Tts(error.to_string()))?;

        let (_, duration_data) = dp_outputs["duration"]
            .try_extract_tensor::<f32>()
            .map_err(|error| AppError::Tts(error.to_string()))?;
        let mut duration = duration_data.to_vec();
        for value in &mut duration {
            *value /= speed;
        }

        let style_ttl_value = OrtValue::from_array(style.ttl.clone())
            .map_err(|error| AppError::Tts(error.to_string()))?;
        let text_enc_outputs = self
            .text_enc_ort
            .run(ort::inputs! {
                "text_ids" => &text_ids_value,
                "style_ttl" => &style_ttl_value,
                "text_mask" => &text_mask_value
            })
            .map_err(|error| AppError::Tts(error.to_string()))?;

        let (text_emb_shape, text_emb_data) = text_enc_outputs["text_emb"]
            .try_extract_tensor::<f32>()
            .map_err(|error| AppError::Tts(error.to_string()))?;
        let text_emb = Array3::from_shape_vec(
            (
                text_emb_shape[0] as usize,
                text_emb_shape[1] as usize,
                text_emb_shape[2] as usize,
            ),
            text_emb_data.to_vec(),
        )
        .map_err(|error| AppError::Tts(error.to_string()))?;

        let (mut xt, latent_mask) = sample_noisy_latent(
            &duration,
            self.sample_rate,
            self.cfgs.ae.base_chunk_size,
            self.cfgs.ttl.chunk_compress_factor,
            self.cfgs.ttl.latent_dim,
        );
        let total_step_array = Array::from_elem(bsz, total_step as f32);

        for step in 0..total_step {
            let current_step_array = Array::from_elem(bsz, step as f32);
            let xt_value = OrtValue::from_array(xt.clone())
                .map_err(|error| AppError::Tts(error.to_string()))?;
            let text_emb_value = OrtValue::from_array(text_emb.clone())
                .map_err(|error| AppError::Tts(error.to_string()))?;
            let latent_mask_value = OrtValue::from_array(latent_mask.clone())
                .map_err(|error| AppError::Tts(error.to_string()))?;
            let text_mask_value = OrtValue::from_array(text_mask.clone())
                .map_err(|error| AppError::Tts(error.to_string()))?;
            let current_step_value = OrtValue::from_array(current_step_array)
                .map_err(|error| AppError::Tts(error.to_string()))?;
            let total_step_value = OrtValue::from_array(total_step_array.clone())
                .map_err(|error| AppError::Tts(error.to_string()))?;

            let vector_est_outputs = self
                .vector_est_ort
                .run(ort::inputs! {
                    "noisy_latent" => &xt_value,
                    "text_emb" => &text_emb_value,
                    "style_ttl" => &style_ttl_value,
                    "latent_mask" => &latent_mask_value,
                    "text_mask" => &text_mask_value,
                    "current_step" => &current_step_value,
                    "total_step" => &total_step_value
                })
                .map_err(|error| AppError::Tts(error.to_string()))?;

            let (denoised_shape, denoised_data) = vector_est_outputs["denoised_latent"]
                .try_extract_tensor::<f32>()
                .map_err(|error| AppError::Tts(error.to_string()))?;
            xt = Array3::from_shape_vec(
                (
                    denoised_shape[0] as usize,
                    denoised_shape[1] as usize,
                    denoised_shape[2] as usize,
                ),
                denoised_data.to_vec(),
            )
            .map_err(|error| AppError::Tts(error.to_string()))?;
        }

        let final_latent_value =
            OrtValue::from_array(xt).map_err(|error| AppError::Tts(error.to_string()))?;
        let vocoder_outputs = self
            .vocoder_ort
            .run(ort::inputs! {
                "latent" => &final_latent_value
            })
            .map_err(|error| AppError::Tts(error.to_string()))?;

        let (_, wav_data) = vocoder_outputs["wav_tts"]
            .try_extract_tensor::<f32>()
            .map_err(|error| AppError::Tts(error.to_string()))?;
        Ok((wav_data.to_vec(), duration))
    }

    fn call(
        &mut self,
        text: &str,
        language: &str,
        style: &Style,
        total_step: usize,
        speed: f32,
        silence_duration: f32,
    ) -> AppResult<(Vec<f32>, f32)> {
        let chunks = chunk_text_for_language(text, language);
        if chunks.is_empty() {
            return Err(AppError::InvalidInput("text is required".into()));
        }

        let mut wav_cat = Vec::new();
        let mut duration_cat = 0.0_f32;
        for (index, chunk) in chunks.iter().enumerate() {
            let language_list = [language.to_string()];
            let (wav, duration) = self.infer(
                std::slice::from_ref(chunk),
                &language_list,
                style,
                total_step,
                speed,
            )?;
            let duration = duration[0];
            let wav_len = (self.sample_rate as f32 * duration) as usize;
            let wav_chunk = &wav[..wav_len.min(wav.len())];

            if index > 0 {
                let silence_len = (silence_duration * self.sample_rate as f32) as usize;
                wav_cat.extend(std::iter::repeat_n(0.0_f32, silence_len));
                duration_cat += silence_duration;
            }
            wav_cat.extend_from_slice(wav_chunk);
            duration_cat += duration;
        }

        Ok((wav_cat, duration_cat))
    }
}

fn sample_noisy_latent(
    duration: &[f32],
    sample_rate: i32,
    base_chunk_size: i32,
    chunk_compress: i32,
    latent_dim: i32,
) -> (Array3<f32>, Array3<f32>) {
    let bsz = duration.len();
    let max_duration = duration
        .iter()
        .fold(0.0_f32, |left, &right| left.max(right));
    let wav_len_max = (max_duration * sample_rate as f32) as usize;
    let wav_lengths = duration
        .iter()
        .map(|duration| (duration * sample_rate as f32) as usize)
        .collect::<Vec<_>>();
    let chunk_size = (base_chunk_size * chunk_compress) as usize;
    let latent_len = wav_len_max.div_ceil(chunk_size);
    let latent_dim = (latent_dim * chunk_compress) as usize;

    let mut noisy_latent = Array3::<f32>::zeros((bsz, latent_dim, latent_len));
    let normal = Normal::new(0.0, 1.0).expect("normal distribution");
    let mut rng = rand::thread_rng();
    for batch in 0..bsz {
        for dimension in 0..latent_dim {
            for time in 0..latent_len {
                noisy_latent[[batch, dimension, time]] = normal.sample(&mut rng);
            }
        }
    }

    let latent_lengths = wav_lengths
        .iter()
        .map(|length| length.div_ceil(chunk_size))
        .collect::<Vec<_>>();
    let latent_mask = length_to_mask(&latent_lengths, latent_len);
    for batch in 0..bsz {
        for dimension in 0..latent_dim {
            for time in 0..latent_len {
                noisy_latent[[batch, dimension, time]] *= latent_mask[[batch, 0, time]];
            }
        }
    }

    (noisy_latent, latent_mask)
}

fn load_cfgs(onnx_dir: &Path) -> AppResult<Config> {
    let file = File::open(onnx_dir.join("tts.json"))?;
    let reader = BufReader::new(file);
    Ok(serde_json::from_reader(reader)?)
}

fn load_voice_style(voice_style_paths: &[PathBuf]) -> AppResult<Style> {
    if voice_style_paths.is_empty() {
        return Err(AppError::InvalidInput("voice style is required".into()));
    }

    let first_file = File::open(&voice_style_paths[0])?;
    let first_reader = BufReader::new(first_file);
    let first_data: VoiceStyleData = serde_json::from_reader(first_reader)?;
    let ttl_dims = &first_data.style_ttl.dims;
    let dp_dims = &first_data.style_dp.dims;
    let ttl_dim1 = ttl_dims[1];
    let ttl_dim2 = ttl_dims[2];
    let dp_dim1 = dp_dims[1];
    let dp_dim2 = dp_dims[2];
    let batch_size = voice_style_paths.len();
    let mut ttl_flat = vec![0.0_f32; batch_size * ttl_dim1 * ttl_dim2];
    let mut dp_flat = vec![0.0_f32; batch_size * dp_dim1 * dp_dim2];

    for (batch_index, path) in voice_style_paths.iter().enumerate() {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let data: VoiceStyleData = serde_json::from_reader(reader)?;

        let ttl_offset = batch_index * ttl_dim1 * ttl_dim2;
        let mut index = 0;
        for batch in &data.style_ttl.data {
            for row in batch {
                for &value in row {
                    ttl_flat[ttl_offset + index] = value;
                    index += 1;
                }
            }
        }

        let dp_offset = batch_index * dp_dim1 * dp_dim2;
        index = 0;
        for batch in &data.style_dp.data {
            for row in batch {
                for &value in row {
                    dp_flat[dp_offset + index] = value;
                    index += 1;
                }
            }
        }
    }

    Ok(Style {
        ttl: Array3::from_shape_vec((batch_size, ttl_dim1, ttl_dim2), ttl_flat)
            .map_err(|error| AppError::Tts(error.to_string()))?,
        dp: Array3::from_shape_vec((batch_size, dp_dim1, dp_dim2), dp_flat)
            .map_err(|error| AppError::Tts(error.to_string()))?,
    })
}

fn load_text_to_speech(onnx_dir: &Path) -> AppResult<TextToSpeech> {
    let cfgs = load_cfgs(onnx_dir)?;
    let dp_ort = Session::builder()
        .map_err(|error| AppError::Tts(error.to_string()))?
        .commit_from_file(onnx_dir.join("duration_predictor.onnx"))
        .map_err(|error| AppError::Tts(error.to_string()))?;
    let text_enc_ort = Session::builder()
        .map_err(|error| AppError::Tts(error.to_string()))?
        .commit_from_file(onnx_dir.join("text_encoder.onnx"))
        .map_err(|error| AppError::Tts(error.to_string()))?;
    let vector_est_ort = Session::builder()
        .map_err(|error| AppError::Tts(error.to_string()))?
        .commit_from_file(onnx_dir.join("vector_estimator.onnx"))
        .map_err(|error| AppError::Tts(error.to_string()))?;
    let vocoder_ort = Session::builder()
        .map_err(|error| AppError::Tts(error.to_string()))?
        .commit_from_file(onnx_dir.join("vocoder.onnx"))
        .map_err(|error| AppError::Tts(error.to_string()))?;
    let text_processor = UnicodeProcessor::new(&onnx_dir.join("unicode_indexer.json"))?;

    Ok(TextToSpeech::new(
        cfgs,
        text_processor,
        dp_ort,
        text_enc_ort,
        vector_est_ort,
        vocoder_ort,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_synthesizer_produces_audio_without_a_model() {
        // The whole point of the seam: this runs with no ONNX runtime, no model
        // directory and no network.
        let mut engine = FakeSynthesizer;

        let samples = engine
            .synthesize("A sentence to speak.", "en", "M1", 1.0)
            .unwrap();

        assert!(!samples.is_empty());
        assert!(samples.iter().all(|sample| sample.abs() <= 1.0));
    }

    #[test]
    fn longer_text_produces_longer_audio() {
        let mut engine = FakeSynthesizer;

        let short = engine.synthesize("Hi.", "en", "M1", 1.0).unwrap();
        let long = engine
            .synthesize(&"word ".repeat(60), "en", "M1", 1.0)
            .unwrap();

        assert!(long.len() > short.len());
    }
}

/// Latency measurement against the real 394 MB model. Ignored by default:
/// it loads the model bundle from the app-data directory, so it needs a real
/// install and is not part of the normal suite.
///
///   cargo test -p libretexts-reader supertonic_latency -- --ignored --nocapture
#[cfg(test)]
mod latency_bench {
    use super::synthesize_samples_blocking;
    use crate::tts::supertonic::audio::SUPERTONIC_SAMPLE_RATE;
    use std::time::Instant;

    struct Timing {
        elapsed: f32,
        realtime_ratio: f32,
    }

    fn report(label: &str, text: &str) -> Timing {
        let started = Instant::now();
        let samples = synthesize_samples_blocking(text, "M1", "en", 1.0).expect("synthesis");
        let elapsed = started.elapsed().as_secs_f32();
        let audio_seconds = samples.len() as f32 / SUPERTONIC_SAMPLE_RATE as f32;
        let realtime_ratio = audio_seconds / elapsed;

        println!(
            "{label:<28} {elapsed:>7.2}s wall  {audio_seconds:>6.2}s audio  \
             {realtime_ratio:>5.2}x realtime  ({} chars)",
            text.chars().count()
        );

        Timing {
            elapsed,
            realtime_ratio,
        }
    }

    #[test]
    #[ignore]
    fn supertonic_latency() {
        let sentence = "The mitochondrion is the powerhouse of the cell.";

        // First call pays the model load; the engine is cached after it.
        let cold = report("cold (first sentence)", sentence);
        let warm = report("warm (same sentence)", sentence);
        report(
            "warm (second sentence)",
            "Photosynthesis converts light energy into chemical energy.",
        );

        let paragraph = "Cells are the basic unit of life. Every living organism is made of \
             one or more cells. Some organisms consist of a single cell, while others contain \
             trillions. The cell theory states that all cells arise from pre-existing cells.";
        let paragraph = report("warm (paragraph)", paragraph);
        let paragraph_ratio = paragraph.realtime_ratio;

        println!(
            "\nmodel load cost: ~{:.2}s (cold minus warm)",
            (cold.elapsed - warm.elapsed).max(0.0)
        );

        // Playback has to outrun the listener or it stalls forever. At
        // opt-level 0 this ran at 0.19x -- five times slower than realtime --
        // which is what "Supertonic takes forever" was. The dev profile now
        // sets opt-level 1 (and 3 for dependencies) precisely so a debug build
        // stays usable; if someone removes that, this is what catches it.
        assert!(
            paragraph_ratio > 1.0,
            "synthesis must outrun realtime, got {paragraph_ratio:.2}x -- \
             check [profile.dev] opt-level in the workspace Cargo.toml"
        );
    }

    #[test]
    #[ignore]
    fn supertonic_repeated_synthesis_memory_probe() {
        let iterations = std::env::var("LIBRETEXTS_SUPERTONIC_MEMORY_ITERATIONS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(20);
        for _ in 0..iterations {
            let samples = synthesize_samples_blocking(
                "Las células utilizan energía para mantener su organización, crecer y reproducirse.",
                "M1",
                "es",
                1.0,
            )
            .expect("synthesis");
            assert!(!samples.is_empty());
        }
        println!("completed {iterations} repeated real-model syntheses");
    }
}
