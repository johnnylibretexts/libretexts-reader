use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};
use uuid::Uuid;

use crate::commands::tts::SpeechAudio;
use crate::content::normalize::normalize_for_tts;
use crate::db::connection::DbPool;
use crate::db::library;
use crate::db::settings;
use crate::error::{AppError, AppResult};
use crate::net::download::{download_verified, Download};
use crate::secrets::{KeyringSecretStore, SecretStore, FISH_KEY_ACCOUNT};
use crate::tts::fish::client::FishClient;
use crate::tts::fish::provider::FishProvider;
use crate::tts::fish::FISH_MODEL;
use crate::tts::provider::TtsProvider;
use crate::tts::supertonic::audio::{encode_f32_to_mp3, encode_f32_to_wav, SUPERTONIC_SAMPLE_RATE};
use crate::tts::supertonic::cache::{
    cache_path_for_chapter, copy_cached_mp3, estimate_for_text, output_path_for_chapter,
    path_to_string,
};
use crate::tts::supertonic::engine;
use crate::tts::supertonic::model::{
    emit_supertonic_model_progress, existing_supertonic_model_bytes, file_complete,
    supertonic_model_dir, supertonic_model_file_path, supertonic_model_manifest,
    supertonic_model_status, temp_download_path, SupertonicModelStatus, SUPERTONIC_MODEL_VERSION,
    SUPERTONIC_READ_TIMEOUT, SUPERTONIC_USER_AGENT,
};
use crate::tts::supertonic::provider::SupertonicProvider;
use crate::tts::supertonic::voice::{
    normalize_language, playback_voice_style, resolve_language, resolve_voice_style,
    DEFAULT_LANGUAGE, DEFAULT_VOICE_STYLE,
};
use crate::tts::supertonic::{
    ChapterEstimate, ChapterMaterial, ChapterRequest, SupertonicConfig, SUPERTONIC_DEFAULT_SPEED,
};

/// Everything a provider needs, read once by the caller.
///
/// A struct rather than a `DbPool` so `provider_for` is pure and testable and
/// cannot reach the database or the keychain itself.
pub struct ProviderSettings {
    pub supertonic_voice_style: String,
    pub supertonic_language: String,
    pub fish_voice_id: Option<String>,
    pub fish_api_key: Option<String>,
}

/// Build the provider the *caller* named.
///
/// `name` comes from the request, never from the `tts_provider` setting. See
/// the doc comment on `commands::tts::synthesize_speech`: the webview is the
/// single place an engine is chosen, and re-deriving that here from a second
/// source with no ordering guarantee is the bug that was removed.
pub fn provider_for(name: &str, settings: &ProviderSettings) -> AppResult<Box<dyn TtsProvider>> {
    match name {
        "supertonic" => Ok(Box::new(SupertonicProvider)),
        "fish" => {
            let key = settings.fish_api_key.clone().ok_or_else(|| {
                AppError::Auth("Add a Fish Audio API key in Settings first.".into())
            })?;
            Ok(Box::new(FishProvider::new(
                FishClient::new(key)?,
                settings.fish_voice_id.clone(),
            )))
        }
        other => Err(AppError::InvalidInput(format!(
            "unknown TTS provider: {other}"
        ))),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupertonicPreviewRequest {
    pub text: String,
    pub voice_style: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterExport {
    pub output_path: String,
    pub cached: bool,
    pub byte_length: u64,
    pub estimate: ChapterEstimate,
}

#[tauri::command]
pub async fn get_supertonic_model_status() -> AppResult<SupertonicModelStatus> {
    supertonic_model_status()
}

#[tauri::command]
pub async fn ensure_supertonic_model_downloaded<R: Runtime>(
    window: Window<R>,
) -> AppResult<String> {
    let manifest = supertonic_model_manifest()?;
    let directory = supertonic_model_dir()?;
    tokio::fs::create_dir_all(&directory).await?;
    let total = manifest
        .files
        .iter()
        .map(|file| file.size_bytes)
        .sum::<u64>();
    let mut downloaded = existing_supertonic_model_bytes(&directory, &manifest.files);
    let client = reqwest::Client::builder()
        .user_agent(SUPERTONIC_USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;

    emit_supertonic_model_progress(&window, "Preparing", downloaded, total)?;
    for file in manifest.files {
        let target_path = supertonic_model_file_path(&directory, &file)?;
        if file_complete(&target_path, file.size_bytes, &file.sha256)? {
            continue;
        }

        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let temp_path = temp_download_path(&target_path)?;
        if temp_path.exists() {
            tokio::fs::remove_file(&temp_path).await?;
        }
        if target_path.exists() {
            tokio::fs::remove_file(&target_path).await?;
        }

        // Size and digest are checked as the bytes arrive; the old loop here
        // wrote the whole file first and then re-read it to hash it.
        let file_size = file.size_bytes;
        let file_label = file.path.clone();
        download_verified(
            &client,
            Download {
                url: &file.url,
                temp_path: &temp_path,
                expected_sha256: &file.sha256,
                expected_size: file_size,
                read_timeout: SUPERTONIC_READ_TIMEOUT,
                error: AppError::Model,
            },
            |file_downloaded, _total| {
                emit_supertonic_model_progress(
                    &window,
                    &file_label,
                    downloaded + file_downloaded,
                    total.max(downloaded + file_downloaded),
                )
            },
        )
        .await?;

        let file_downloaded = file_size;
        downloaded += file_downloaded;
        tokio::fs::rename(&temp_path, &target_path).await?;
    }

    let status = supertonic_model_status()?;
    if !status.downloaded {
        return Err(AppError::Model(
            "Supertonic model download is incomplete.".into(),
        ));
    }
    emit_supertonic_model_progress(
        &window,
        "Complete",
        status.downloaded_bytes,
        status.total_bytes,
    )?;
    Ok(status.directory)
}

#[tauri::command]
pub async fn preview_supertonic_tts(
    state: State<'_, DbPool>,
    request: SupertonicPreviewRequest,
) -> AppResult<SpeechAudio> {
    let text = normalize_for_tts(request.text.trim());
    if text.is_empty() {
        return Err(AppError::InvalidInput("preview text is required".into()));
    }

    let config = supertonic_config_from_state(&state)?;
    let voice_style = resolve_voice_style(request.voice_style.as_deref(), &config.voice_style)?;
    let language = resolve_language(request.language.as_deref(), &config.language)?;

    synthesize_supertonic_audio(text, voice_style, language, SUPERTONIC_DEFAULT_SPEED).await
}

/// Everything both chapter commands need to know before they diverge.
///
/// The two used to work this out separately from identical eight-line
/// prologues, and export then recomputed the estimate the frontend had just
/// asked for. Resolving it once means the estimate a reader is shown and the
/// one returned by the export are the same value, not two that happen to agree.
struct ChapterJob {
    material: ChapterMaterial,
    voice_style: String,
    language: String,
    output_path: PathBuf,
    cache_path: PathBuf,
    estimate: ChapterEstimate,
}

/// The cache-key model string for a provider, kept in one place so it can
/// only ever be `SUPERTONIC_MODEL_VERSION` or `FISH_MODEL` — the same
/// constants `provider_for` and the two engines' own tests already pin.
///
/// Separate from `provider_for`'s own match: this only needs a name, not a
/// `ProviderSettings`, so both the estimate and export commands can call it
/// without touching the database or the keychain.
fn model_for_provider(provider: &str) -> AppResult<&'static str> {
    match provider {
        "supertonic" => Ok(SUPERTONIC_MODEL_VERSION),
        "fish" => Ok(FISH_MODEL),
        other => Err(AppError::InvalidInput(format!(
            "unknown TTS provider: {other}"
        ))),
    }
}

/// Voice and language resolution, kept provider-aware.
///
/// `resolve_voice_style` / `resolve_language` only know Supertonic's ten
/// voice styles and closed language list. Running a Fish request through
/// them rejected every real Fish voice id -- a `reference_id` is an opaque
/// model id (e.g. `d8ee9d1a...`, or a public model id pasted from
/// fish.audio) that looks nothing like `M1`..`F5` -- and required a language
/// code Fish does not take, since Fish infers language from the text across
/// 83 languages instead. This match is the one place that fork happens: the
/// `_` arm below calls the exact same two functions with the exact same
/// arguments as before this fix, so Supertonic's behaviour is unchanged.
///
/// `model_for_provider` is called before this in `resolve_chapter_job`, so an
/// unrecognised provider is already rejected by the time this runs; the `_`
/// arm only ever sees `"supertonic"` in practice.
fn resolve_voice_and_language(
    provider: &str,
    request: &ChapterRequest,
    config: &SupertonicConfig,
) -> AppResult<(String, String)> {
    match provider {
        "fish" => {
            let voice_style = request
                .voice_style
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| {
                    AppError::Voice("Choose a Fish Audio voice in Settings before using it.".into())
                })?;
            // No validation: Fish infers language from the text itself. A
            // value is still needed for chunking the estimate and for the
            // content-addressed cache key, so a blank request falls back to
            // Supertonic's own default rather than being rejected.
            let language = request
                .language
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(DEFAULT_LANGUAGE)
                .to_string();
            Ok((voice_style, language))
        }
        _ => Ok((
            resolve_voice_style(request.voice_style.as_deref(), &config.voice_style)?,
            resolve_language(request.language.as_deref(), &config.language)?,
        )),
    }
}

/// How many characters this export will actually be billed for.
///
/// Zero for a local provider, always. For Fish: zero when the chapter is
/// cached AND the caller is not forcing a re-synthesis, because a plain
/// (non-forced) export of a cached chapter is served from the cache file and
/// never reaches the network. But `export_supertonic_chapter_mp3` honours
/// `force` by skipping the cache and re-synthesising regardless of whether a
/// cached copy exists -- a real, billed request -- so `cached` alone is not
/// enough to decide this. A forced Fish export of an already-cached chapter
/// bills its full character count, exactly like an uncached one.
pub fn billable_characters(text: &str, provider: &str, cached: bool, force: bool) -> u32 {
    if provider != "fish" || (cached && !force) {
        return 0;
    }
    text.chars().count() as u32
}

fn resolve_chapter_job(
    state: &State<'_, DbPool>,
    request: &ChapterRequest,
) -> AppResult<ChapterJob> {
    let config = supertonic_config_from_state(state)?;
    let material = chapter_material(state, &request.document_id, &request.section_id)?;
    let model = model_for_provider(&request.provider)?;
    let (voice_style, language) = resolve_voice_and_language(&request.provider, request, &config)?;
    let output_path = output_path_for_chapter(&config, &material, &voice_style, &language, request);
    let cache_path =
        cache_path_for_chapter(&request.provider, model, &material, &voice_style, &language)?;
    let mut estimate = estimate_for_text(
        &material,
        &language,
        &output_path,
        cache_path.exists(),
        &request.provider,
    );
    estimate.billable_characters = billable_characters(
        &material.text,
        &request.provider,
        estimate.cached,
        request.force.unwrap_or(false),
    );

    Ok(ChapterJob {
        material,
        voice_style,
        language,
        output_path,
        cache_path,
        estimate,
    })
}

#[tauri::command]
pub async fn estimate_supertonic_chapter(
    state: State<'_, DbPool>,
    request: ChapterRequest,
) -> AppResult<ChapterEstimate> {
    Ok(resolve_chapter_job(&state, &request)?.estimate)
}

#[tauri::command]
pub async fn export_supertonic_chapter_mp3(
    state: State<'_, DbPool>,
    request: ChapterRequest,
) -> AppResult<ChapterExport> {
    let job = resolve_chapter_job(&state, &request)?;
    let force = request.force.unwrap_or(false);

    if job.cache_path.exists() && !force {
        copy_cached_mp3(&job.cache_path, &job.output_path).await?;
        let bytes = tokio::fs::metadata(&job.output_path).await?.len();
        return Ok(ChapterExport {
            output_path: path_to_string(&job.output_path),
            cached: true,
            byte_length: bytes,
            estimate: job.estimate,
        });
    }

    if let Some(parent) = job.cache_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let settings = provider_settings_from_state(&state)?;
    let provider = provider_for(&request.provider, &settings)?;
    // Export always renders at one fixed speed -- chosen here, not buried in
    // the trait, since a per-request speed makes no sense for a file that is
    // written once and played back later at whatever speed the reader picks.
    let mp3 = provider
        .synthesize(
            &job.material.text,
            &job.voice_style,
            &job.language,
            SUPERTONIC_DEFAULT_SPEED,
        )
        .await?;
    if mp3.is_empty() {
        return Err(AppError::Tts(format!(
            "{} returned empty audio.",
            provider.id()
        )));
    }

    // Unique temp name so concurrent exports of the same chapter cannot write
    // to the same in-progress file before the atomic rename into place.
    let temp_path = job
        .cache_path
        .with_extension(format!("{}.mp3.download", Uuid::new_v4()));
    tokio::fs::write(&temp_path, &mp3).await?;
    tokio::fs::rename(&temp_path, &job.cache_path).await?;
    copy_cached_mp3(&job.cache_path, &job.output_path).await?;

    Ok(ChapterExport {
        output_path: path_to_string(&job.output_path),
        cached: false,
        byte_length: mp3.len() as u64,
        // The chapter was not cached when the job was resolved, which is why
        // it was synthesized; it is now, so nothing further is billable for
        // it.
        estimate: ChapterEstimate {
            cached: true,
            billable_characters: 0,
            ..job.estimate
        },
    })
}

/// Playback synthesis, driven entirely by what the caller passes.
///
/// Deliberately lenient about voice and language rather than erroring the way
/// the preview and export commands do: the player carries one voice id across
/// engines, so a reader who switches engines mid-session can briefly hold an
/// id this engine does not know. Falling back beats cutting the audio off.
/// User-initiated commands still reject unknown values outright.
pub async fn synthesize_supertonic_text(
    text: &str,
    voice_id: Option<&str>,
    language: Option<&str>,
    speed: f32,
) -> AppResult<SpeechAudio> {
    let voice_style = playback_voice_style(voice_id, DEFAULT_VOICE_STYLE);
    let language = normalize_language(language, DEFAULT_LANGUAGE).to_string();

    synthesize_supertonic_audio(
        normalize_for_tts(text),
        voice_style,
        language,
        clamp_speed(speed),
    )
    .await
}

fn supertonic_config_from_state(state: &State<'_, DbPool>) -> AppResult<SupertonicConfig> {
    let conn = state.get()?;
    SupertonicConfig::from_settings(&settings::get_all_settings(&conn)?)
}

/// Read what `provider_for` needs from the database and the keychain.
///
/// Kept out of `provider_for` itself so that function stays pure: this is the
/// one place a command reaches the DB pool and `KeyringSecretStore` before
/// handing plain values to the dispatcher.
pub(crate) fn provider_settings_from_state(
    state: &State<'_, DbPool>,
) -> AppResult<ProviderSettings> {
    let conn = state.get()?;
    let values = settings::get_all_settings(&conn)?;
    let config = SupertonicConfig::from_settings(&values)?;
    let fish_voice_id = crate::tts::supertonic::optional_setting_string(&values, "fish_voice_id");
    let fish_api_key = KeyringSecretStore::new(FISH_KEY_ACCOUNT).get()?;

    Ok(ProviderSettings {
        supertonic_voice_style: config.voice_style,
        supertonic_language: config.language,
        fish_voice_id,
        fish_api_key,
    })
}

fn chapter_material(
    state: &State<'_, DbPool>,
    document_id: &str,
    section_id: &str,
) -> AppResult<ChapterMaterial> {
    let conn = state.get()?;
    let document = library::get_document(&conn, document_id)?;
    let section = library::list_sections(&conn, document_id)?
        .into_iter()
        .find(|section| section.id == section_id)
        .ok_or_else(|| AppError::InvalidInput("section does not belong to document".into()))?;
    let paragraphs = library::list_paragraphs(&conn, section_id)?;
    let text = paragraphs
        .into_iter()
        .map(|paragraph| normalize_for_tts(paragraph.text.trim()))
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    if text.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "chapter has no readable text".into(),
        ));
    }

    Ok(ChapterMaterial {
        document,
        section,
        text,
    })
}

async fn synthesize_supertonic_audio(
    text: String,
    voice_style: String,
    language: String,
    speed: f32,
) -> AppResult<SpeechAudio> {
    let samples = synthesize_supertonic_samples(text, voice_style, language, speed).await?;
    let audio = encode_f32_to_wav(&samples, SUPERTONIC_SAMPLE_RATE)?;

    Ok(SpeechAudio {
        audio,
        mime_type: "audio/wav".to_string(),
    })
}

pub(crate) async fn synthesize_supertonic_mp3(
    text: String,
    voice_style: String,
    language: String,
    speed: f32,
) -> AppResult<Vec<u8>> {
    let samples = synthesize_supertonic_samples(text, voice_style, language, speed).await?;
    encode_f32_to_mp3(&samples, SUPERTONIC_SAMPLE_RATE)
}

async fn synthesize_supertonic_samples(
    text: String,
    voice_style: String,
    language: String,
    speed: f32,
) -> AppResult<Vec<f32>> {
    tokio::task::spawn_blocking(move || {
        engine::synthesize_samples_blocking(&text, &voice_style, &language, speed)
    })
    .await
    .map_err(|error| AppError::Tts(format!("Supertonic runtime task failed: {error}")))?
}

fn clamp_speed(speed: f32) -> f32 {
    speed.clamp(0.5, 2.0)
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn dispatch_reads_the_request_not_the_settings_table() {
        let settings = ProviderSettings {
            supertonic_voice_style: "M1".into(),
            supertonic_language: "en".into(),
            fish_voice_id: None,
            fish_api_key: Some("sk-test".into()),
        };

        assert_eq!(
            provider_for("supertonic", &settings).unwrap().id(),
            "supertonic"
        );
        assert_eq!(provider_for("fish", &settings).unwrap().id(), "fish");
    }

    #[test]
    fn an_unknown_provider_is_rejected_rather_than_defaulted() {
        // Falling back to a default would let a frontend bug silently switch
        // engines, which is the class of bug the single-decision-point rule
        // in commands/tts.rs exists to prevent.
        let settings = ProviderSettings {
            supertonic_voice_style: "M1".into(),
            supertonic_language: "en".into(),
            fish_voice_id: None,
            fish_api_key: None,
        };
        assert!(provider_for("kokoro", &settings).is_err());
    }

    #[test]
    fn fish_without_a_key_is_an_auth_error_not_a_panic() {
        let settings = ProviderSettings {
            supertonic_voice_style: "M1".into(),
            supertonic_language: "en".into(),
            fish_voice_id: Some("voice-1".into()),
            fish_api_key: None,
        };
        assert_eq!(provider_for("fish", &settings).unwrap_err().kind(), "auth");
    }
}

#[cfg(test)]
mod billable_tests {
    use super::billable_characters;

    #[test]
    fn a_cached_chapter_bills_nothing() {
        // The gate must not ask the user to approve spending on audio that
        // already exists on disk -- as long as the caller is not about to
        // force a re-synthesis of it (see the forced case below).
        assert_eq!(
            billable_characters("Some text here.", "fish", true, false),
            0
        );
    }

    #[test]
    fn an_uncached_fish_chapter_bills_its_characters() {
        assert_eq!(
            billable_characters("Some text here.", "fish", false, false),
            15
        );
    }

    #[test]
    fn a_forced_export_of_a_cached_fish_chapter_bills_its_full_count() {
        // The regression guard for the billing bypass: `export_supertonic_
        // chapter_mp3` honours `force` by re-synthesising even a chapter
        // that is already cached -- a real, billed Fish request. If this
        // function looked at `cached` alone, a forced "Regenerate" of an
        // already-exported Fish chapter would report 0 billable characters
        // and skip the frontend's confirmation gate entirely.
        assert_eq!(
            billable_characters("Some text here.", "fish", true, true),
            15
        );
    }

    #[test]
    fn a_forced_supertonic_regeneration_still_bills_nothing() {
        // Force never matters for a local provider: there is no network
        // request to gate in the first place.
        assert_eq!(
            billable_characters("Some text here.", "supertonic", true, true),
            0
        );
    }

    #[test]
    fn supertonic_never_bills() {
        assert_eq!(
            billable_characters("Some text here.", "supertonic", false, false),
            0
        );
    }

    #[test]
    fn counts_characters_not_bytes() {
        // Fish bills text, and a multi-byte character is one character. Using
        // len() here would overstate an accented or CJK chapter by 2-3x.
        assert_eq!(billable_characters("héllo", "fish", false, false), 5);
    }
}

/// The Task 5 blocker: `resolve_chapter_job` used to run Supertonic-only
/// voice/language validation unconditionally, rejecting every Fish chapter
/// export or estimate before it started. These tests exercise
/// `resolve_voice_and_language` directly -- the pure seam both commands
/// funnel through -- since a full `resolve_chapter_job` call needs a Tauri
/// `State<DbPool>`, which is out of reach for a unit test.
#[cfg(test)]
mod provider_aware_validation_tests {
    use super::*;

    fn config() -> SupertonicConfig {
        SupertonicConfig {
            voice_style: DEFAULT_VOICE_STYLE.to_string(),
            language: DEFAULT_LANGUAGE.to_string(),
            export_directory: "/tmp".to_string(),
        }
    }

    fn request(
        provider: &str,
        voice_style: Option<&str>,
        language: Option<&str>,
    ) -> ChapterRequest {
        ChapterRequest {
            document_id: "doc-1".into(),
            section_id: "sec-1".into(),
            provider: provider.to_string(),
            voice_style: voice_style.map(str::to_string),
            language: language.map(str::to_string),
            output_path: None,
            force: None,
        }
    }

    #[test]
    fn a_fish_reference_id_supertonic_would_reject_still_resolves() {
        // This is the regression guard for the whole fix: without it, a
        // future change could reintroduce the unconditional Supertonic
        // validation and every test in this module could still pass if it
        // only used ids that happen to look like Supertonic voice styles.
        // A real Fish reference_id looks nothing like `M1`..`F5`.
        let voice_id = "d8ee9d1a-6f3e-4b8a-9c1d-abcdef012345";
        assert!(
            resolve_voice_style(Some(voice_id), DEFAULT_VOICE_STYLE).is_err(),
            "the id must actually be one Supertonic's validator rejects, or this test proves nothing"
        );

        let (resolved_voice, _language) =
            resolve_voice_and_language("fish", &request("fish", Some(voice_id), None), &config())
                .unwrap();
        assert_eq!(resolved_voice, voice_id);
    }

    #[test]
    fn fish_without_a_configured_voice_is_a_voice_error_not_a_panic() {
        let error = resolve_voice_and_language("fish", &request("fish", None, None), &config())
            .unwrap_err();
        assert_eq!(error.kind(), "voice");
    }

    #[test]
    fn fish_ignores_supertonic_language_validation() {
        // "klingon" would fail resolve_language outright; Fish must not care,
        // since it infers language from the text across 83 languages rather
        // than taking a language code.
        assert!(resolve_language(Some("klingon"), DEFAULT_LANGUAGE).is_err());

        let (_voice, language) = resolve_voice_and_language(
            "fish",
            &request("fish", Some("voice-1"), Some("klingon")),
            &config(),
        )
        .unwrap();
        assert_eq!(language, "klingon");
    }

    #[test]
    fn fish_blank_language_falls_back_instead_of_erroring() {
        let (_voice, language) =
            resolve_voice_and_language("fish", &request("fish", Some("voice-1"), None), &config())
                .unwrap();
        assert_eq!(language, DEFAULT_LANGUAGE);
    }

    #[test]
    fn supertonic_validation_is_byte_for_byte_unchanged() {
        let (voice, language) = resolve_voice_and_language(
            "supertonic",
            &request("supertonic", Some("f2"), Some("fr")),
            &config(),
        )
        .unwrap();
        assert_eq!(voice, "F2");
        assert_eq!(language, "fr");

        assert!(resolve_voice_and_language(
            "supertonic",
            &request("supertonic", Some("not-a-real-voice"), None),
            &config()
        )
        .is_err());
        assert!(resolve_voice_and_language(
            "supertonic",
            &request("supertonic", None, Some("klingon")),
            &config()
        )
        .is_err());
    }
}
