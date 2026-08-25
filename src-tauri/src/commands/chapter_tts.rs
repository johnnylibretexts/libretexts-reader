use std::path::PathBuf;

use rusqlite::Connection;
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
use crate::tts::supertonic::audio::{encode_f32_to_m4a, encode_f32_to_wav, SUPERTONIC_SAMPLE_RATE};
use crate::tts::supertonic::cache::{
    cache_path_for_chapter, copy_cached_export, estimate_for_text, output_path_for_chapter,
    path_to_string,
};
use crate::tts::supertonic::engine;
use crate::tts::supertonic::model::{
    emit_supertonic_model_progress, existing_supertonic_model_bytes, file_complete,
    supertonic_model_dir, supertonic_model_file_path, supertonic_model_manifest,
    supertonic_model_status, temp_download_path, SupertonicDownload, SupertonicDownloadCancel,
    SupertonicModelStatus, SUPERTONIC_MODEL_VERSION, SUPERTONIC_READ_TIMEOUT,
    SUPERTONIC_USER_AGENT,
};
use crate::tts::supertonic::provider::SupertonicProvider;
use crate::tts::supertonic::voice::{
    normalize_language, playback_voice_style, resolve_language, resolve_voice_style,
    DEFAULT_LANGUAGE, DEFAULT_VOICE_STYLE,
};
use crate::tts::supertonic::{
    ChapterEstimate, ChapterMaterial, ChapterRequest, SupertonicConfig, SUPERTONIC_DEFAULT_SPEED,
};
use crate::tts::tags::tag_chapter_export;

/// Everything a provider needs, read once by the caller.
///
/// A struct rather than a `DbPool` so `provider_for` is pure and testable and
/// cannot reach the database or the keychain itself.
pub struct ProviderSettings {
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

/// Fetch the Supertonic model, or join the fetch already in flight.
///
/// Two surfaces call this -- the player on first Play and the Settings
/// Download button -- and they used to be able to run at the same time, both
/// clearing the same cancel flag and both writing the same `.download` temp
/// paths. `SupertonicDownload::run` makes it single-flight: the second caller
/// waits for the first and is handed its result.
#[tauri::command]
pub async fn ensure_supertonic_model_downloaded<R: Runtime>(
    window: Window<R>,
    download: State<'_, SupertonicDownload>,
) -> AppResult<String> {
    let download = SupertonicDownload::clone(&download);
    download
        .run(|cancel| async move { fetch_supertonic_model(&window, &cancel).await })
        .await
}

/// The download itself, given the cancel handle of the run it belongs to.
///
/// Takes that handle as an argument rather than reading managed state, so the
/// flag this checks is necessarily the one `run` cleared when it started.
async fn fetch_supertonic_model<R: Runtime>(
    window: &Window<R>,
    cancel: &SupertonicDownloadCancel,
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

    emit_supertonic_model_progress(window, "Preparing", downloaded, total)?;
    for file in manifest.files {
        let target_path = supertonic_model_file_path(&directory, &file)?;
        if file_complete(&target_path, file.size_bytes, &file.sha256)? {
            continue;
        }

        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // The partial is deliberately left where it is: `download_verified`
        // resumes it with a `Range` request, so a drop or a Cancel partway
        // through the 256 MB file no longer costs the reader the whole file.
        let temp_path = temp_download_path(&target_path)?;
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
                // Checked here because `download_verified` calls this on every
                // chunk and `?`-propagates the result: an error drops the HTTP
                // stream immediately instead of finishing the 256MB file first.
                cancel.check()?;
                emit_supertonic_model_progress(
                    window,
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
        window,
        "Complete",
        status.downloaded_bytes,
        status.total_bytes,
    )?;
    Ok(status.directory)
}

/// Stop the model download in flight.
///
/// Returns as soon as the flag is set -- the download itself fails on its next
/// chunk, from inside `fetch_supertonic_model`. Setting the flag with no
/// download running is harmless: the next one to *start* clears it, and a
/// caller that merely joins a running download does not, which is what stopped
/// a Settings request from voiding a Cancel already pressed.
#[tauri::command]
pub async fn cancel_supertonic_model_download(
    download: State<'_, SupertonicDownload>,
) -> AppResult<()> {
    download.request_cancel();
    Ok(())
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
    pub target_lang: Option<String>,
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
    let target_lang = saved_translation_target(state)?;
    let material = chapter_material(
        state,
        &request.document_id,
        &request.section_id,
        target_lang.as_deref(),
    )?;
    let model = model_for_provider(&request.provider)?;
    let (voice_style, requested_language) =
        resolve_voice_and_language(&request.provider, request, &config)?;
    // One setting owns both translation and pronunciation. Even a stale
    // frontend request cannot send Spanish text through English rules.
    let language = target_lang.clone().unwrap_or(requested_language);
    let output_path = output_path_for_chapter(
        &config,
        &material,
        &voice_style,
        &language,
        target_lang.as_deref(),
        request,
    );
    let cache_path = cache_path_for_chapter(
        &request.provider,
        model,
        &material,
        &voice_style,
        &language,
        target_lang.as_deref(),
    )?;
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
        target_lang,
        output_path,
        cache_path,
        estimate,
    })
}

/// Write bytes into place via a uniquely-named temp file.
///
/// The unique name is what stops two concurrent exports of the same chapter
/// from writing to one in-progress file before the atomic rename.
async fn write_atomic(bytes: &[u8], path: &std::path::Path) -> AppResult<()> {
    let temp_path = path.with_extension(format!("{}.download", Uuid::new_v4()));
    tokio::fs::write(&temp_path, bytes).await?;
    tokio::fs::rename(&temp_path, path).await?;
    Ok(())
}

/// Put the audio where the reader asked for it, and cache it if we can.
///
/// The cache is an optimization; the reader's file is what they paid for. A
/// paid provider has already billed for these bytes by the time we hold them,
/// so a cache-side failure -- a full app-data volume, a cache directory that
/// vanished -- must not abort the export. Doing so drops the only copy of
/// billed audio and makes the retry bill again, indefinitely if the cache
/// problem persists. A failure writing the reader's own file is still fatal:
/// there is nothing left to deliver.
async fn deliver_synthesized_export(
    audio: &[u8],
    cache_path: &std::path::Path,
    output_path: &std::path::Path,
) -> AppResult<()> {
    if let Err(error) = write_atomic(audio, cache_path).await {
        eprintln!("chapter export: caching failed, delivering uncached: {error}");

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        return write_atomic(audio, output_path).await;
    }

    copy_cached_export(cache_path, output_path).await
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
        copy_cached_export(&job.cache_path, &job.output_path).await?;
        // Tagged on the way out rather than in the cache, so a chapter cached
        // before tagging existed still leaves with its credit attached, and
        // the byte length below is the file the reader actually gets.
        tag_chapter_export(
            &job.output_path,
            &job.material.document,
            &job.material.section,
            job.target_lang.as_deref(),
        )?;
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

    let settings = provider_settings_from_state(&state, &request.provider)?;
    let provider = provider_for(&request.provider, &settings)?;
    // Export always renders at one fixed speed -- chosen here, not buried in
    // the trait, since a per-request speed makes no sense for a file that is
    // written once and played back later at whatever speed the reader picks.
    let audio = provider
        .synthesize(
            &job.material.text,
            &job.voice_style,
            &job.language,
            SUPERTONIC_DEFAULT_SPEED,
        )
        .await?;
    if audio.is_empty() {
        return Err(AppError::Tts(format!(
            "{} returned empty audio.",
            provider.id()
        )));
    }

    deliver_synthesized_export(&audio, &job.cache_path, &job.output_path).await?;
    tag_chapter_export(
        &job.output_path,
        &job.material.document,
        &job.material.section,
        job.target_lang.as_deref(),
    )?;
    // The file on disk, not the synthesized buffer: the tag added above is
    // part of what the reader receives.
    let byte_length = tokio::fs::metadata(&job.output_path).await?.len();

    Ok(ChapterExport {
        output_path: path_to_string(&job.output_path),
        cached: false,
        byte_length,
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
/// the preview and export commands do: playback sends whatever the settings
/// row holds, which may be a style or language retired between releases, and
/// falling back beats cutting the audio off mid-chapter. User-initiated
/// commands still reject unknown values outright. Same split, and same
/// reasoning, as the module doc on `tts::supertonic::voice`.
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
///
/// Takes the provider the caller asked for so the keychain is only touched
/// when the answer can matter — see `fish_api_key_for`.
pub(crate) fn provider_settings_from_state(
    state: &State<'_, DbPool>,
    provider: &str,
) -> AppResult<ProviderSettings> {
    let conn = state.get()?;
    let values = settings::get_all_settings(&conn)?;
    // Supertonic's config is deliberately not parsed here. It was, only to
    // fill two fields nothing read, and `SupertonicConfig::from_settings` is
    // fallible -- so a corrupt Supertonic voice or language failed a Fish
    // request that never consults either. Supertonic's own path parses it
    // where it is actually used (`supertonic_config_from_state`).
    let fish_voice_id = crate::tts::supertonic::optional_setting_string(&values, "fish_voice_id");
    let fish_api_key = fish_api_key_for(provider, &KeyringSecretStore::new(FISH_KEY_ACCOUNT))?;

    Ok(ProviderSettings {
        fish_voice_id,
        fish_api_key,
    })
}

/// The Fish key, read only when a Fish request actually needs it.
///
/// This used to be an unconditional `KeyringSecretStore::get()?` on the way
/// to every synthesis, including Supertonic's. A keychain that cannot be read
/// — no secret-service daemon on Linux, a locked or access-denied keychain on
/// macOS — is an `AppError::Auth`, and the `?` turned that into a hard failure
/// of the bundled offline engine for a user who has no Fish key and never
/// asked for one. The README promises Supertonic works with "no account, no
/// key, no network"; an optional cloud provider must not be able to take that
/// away.
///
/// So the read is scoped to the one provider that can use the result. A Fish
/// request still surfaces the keychain error, because for Fish it is the real
/// reason the request cannot proceed.
///
/// Takes `&dyn SecretStore` rather than calling the keyring directly for the
/// same reason `cache_path_in` takes a root: a test can hand it a failing
/// store without going near the developer's login keychain.
pub(crate) fn fish_api_key_for(
    provider: &str,
    store: &dyn SecretStore,
) -> AppResult<Option<String>> {
    if provider != "fish" {
        return Ok(None);
    }

    store.get()
}

fn chapter_material(
    state: &State<'_, DbPool>,
    document_id: &str,
    section_id: &str,
    target_lang: Option<&str>,
) -> AppResult<ChapterMaterial> {
    let conn = state.get()?;
    chapter_material_from_conn(&conn, document_id, section_id, target_lang)
}

fn chapter_material_from_conn(
    conn: &Connection,
    document_id: &str,
    section_id: &str,
    target_lang: Option<&str>,
) -> AppResult<ChapterMaterial> {
    let document = library::get_document(conn, document_id)?;
    let section = library::list_sections(conn, document_id)?
        .into_iter()
        .find(|section| section.id == section_id)
        .ok_or_else(|| AppError::InvalidInput("section does not belong to document".into()))?;
    let paragraphs = library::list_paragraphs(conn, section_id, target_lang)?;
    let text = paragraphs
        .into_iter()
        // `sentence_speech` is the translated-or-fallback form, parallel to
        // the display offsets. Reading `paragraph.text` here would throw the
        // lookup away and export the original chapter again.
        .map(|paragraph| paragraph.sentence_speech.join(" "))
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

fn saved_translation_target(state: &State<'_, DbPool>) -> AppResult<Option<String>> {
    let conn = state.get()?;
    Ok(settings::get_setting(&conn, "translation_target_lang")?
        .and_then(|value| value.as_str().map(str::trim).map(str::to_lowercase))
        .filter(|value| !value.is_empty() && value != "original"))
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

pub(crate) async fn synthesize_supertonic_export(
    text: String,
    voice_style: String,
    language: String,
    speed: f32,
) -> AppResult<Vec<u8>> {
    let samples = synthesize_supertonic_samples(text, voice_style, language, speed).await?;
    encode_f32_to_m4a(&samples, SUPERTONIC_SAMPLE_RATE)
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
            fish_voice_id: None,
            fish_api_key: None,
        };
        assert!(provider_for("kokoro", &settings).is_err());
    }

    #[test]
    fn fish_without_a_key_is_an_auth_error_not_a_panic() {
        let settings = ProviderSettings {
            fish_voice_id: Some("voice-1".into()),
            fish_api_key: None,
        };
        assert_eq!(provider_for("fish", &settings).unwrap_err().kind(), "auth");
    }

    /// A keychain that refuses every read: no secret-service daemon on Linux,
    /// a locked or access-denied login keychain on macOS. Injected rather
    /// than provoked, so this test never goes near the real keychain.
    #[derive(Debug)]
    struct FailingSecretStore;

    impl SecretStore for FailingSecretStore {
        fn set(&self, _secret: &str) -> AppResult<()> {
            Err(AppError::Auth("cannot open the system keychain".into()))
        }

        fn get(&self) -> AppResult<Option<String>> {
            Err(AppError::Auth("cannot open the system keychain".into()))
        }

        fn clear(&self) -> AppResult<()> {
            Err(AppError::Auth("cannot open the system keychain".into()))
        }
    }

    #[test]
    fn a_supertonic_request_survives_a_keychain_that_cannot_be_read() {
        // The whole point of the bundled engine: it works with no account, no
        // key and no network. Reading the Fish key on the way to every
        // synthesis made a keychain failure abort Supertonic playback for a
        // user who has no Fish key and never wanted one.
        let key = fish_api_key_for("supertonic", &FailingSecretStore)
            .expect("a Supertonic request must not read the Fish key at all");
        assert_eq!(key, None);

        let settings = ProviderSettings {
            fish_voice_id: None,
            fish_api_key: key,
        };
        assert_eq!(
            provider_for("supertonic", &settings).unwrap().id(),
            "supertonic"
        );
    }

    #[test]
    fn a_fish_request_still_surfaces_the_keychain_failure() {
        // Scoping the read must not swallow it: for Fish the keychain error
        // is the actual reason the request cannot proceed.
        assert_eq!(
            fish_api_key_for("fish", &FailingSecretStore)
                .unwrap_err()
                .kind(),
            "auth"
        );
    }

    #[test]
    fn a_fish_request_reads_the_key_it_was_given() {
        let store = crate::secrets::MemorySecretStore::default();
        store.set("sk-test").expect("set");

        assert_eq!(
            fish_api_key_for("fish", &store).expect("read"),
            Some("sk-test".to_string())
        );
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

    #[test]
    fn billing_counts_the_characters_actually_sent() {
        let english = "The cell divides.";
        let spanish = "La célula se divide.";

        assert_ne!(
            billable_characters(english, "fish", false, false),
            billable_characters(spanish, "fish", false, false),
        );
    }
}

#[cfg(test)]
mod translated_material_tests {
    use super::*;
    use crate::db::migrations::apply_migrations;

    #[test]
    fn chapter_material_and_billing_use_translated_speech_with_per_sentence_fallback() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_migrations(&mut conn).unwrap();
        conn.execute_batch(
            "INSERT INTO documents
                  (id, title, source_type, source_metadata, word_count, imported_at, source_language)
                  VALUES
                  ('doc-1', 'Biology', 'openstax', '{}', 5, '2026-01-01T00:00:00Z', 'en');
             INSERT INTO sections (id, document_id, ordinal, title, word_count)
                  VALUES ('sec-1', 'doc-1', 0, 'Mitosis', 5);
             INSERT INTO paragraphs (id, section_id, ordinal, text, sentence_offsets)
                  VALUES ('para-1', 'sec-1', 0,
                          'The cell divides. Mitosis begins.',
                          '[[0,17],[18,33]]');
             INSERT INTO sentence_translations
                  (paragraph_id, sentence_index, target_lang, text, qa_status)
                  VALUES ('para-1', 0, 'es', 'La célula se divide.', 'passed');",
        )
        .unwrap();

        let original = chapter_material_from_conn(&conn, "doc-1", "sec-1", None).unwrap();
        let spanish = chapter_material_from_conn(&conn, "doc-1", "sec-1", Some("es")).unwrap();

        assert_eq!(original.text, "The cell divides. Mitosis begins.");
        assert_eq!(spanish.text, "La célula se divide. Mitosis begins.");
        assert_eq!(
            billable_characters(&spanish.text, "fish", false, false),
            spanish.text.chars().count() as u32,
        );
        assert_ne!(
            billable_characters(&original.text, "fish", false, false),
            billable_characters(&spanish.text, "fish", false, false),
        );
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

/// Delivery of already-synthesized audio, exercised on a real filesystem.
///
/// Every path is passed in explicitly and lands under the OS temp dir, so
/// nothing here touches `paths::` or the real app-data tree.
#[cfg(test)]
mod delivery_tests {
    use super::*;

    fn scratch_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("libretexts-reader-{label}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[tokio::test]
    async fn a_normal_export_writes_both_the_cache_and_the_output() {
        let root = scratch_dir("deliver-ok");
        let cache_path = root.join("cache/chapter.mp3");
        let output_path = root.join("out/Chapter One.mp3");
        std::fs::create_dir_all(cache_path.parent().unwrap()).expect("cache dir");

        deliver_synthesized_export(b"paid-audio", &cache_path, &output_path)
            .await
            .expect("delivery must succeed");

        assert_eq!(std::fs::read(&cache_path).expect("cache"), b"paid-audio");
        assert_eq!(std::fs::read(&output_path).expect("output"), b"paid-audio");

        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn a_cache_failure_still_delivers_the_audio_the_reader_paid_for() {
        // Fish has already synthesized and billed for these bytes. The cache is
        // an optimization; the reader's file is the thing they paid for. If a
        // cache-side failure aborts the export, the bytes are dropped and the
        // retry bills again -- indefinitely, if the cache problem persists.
        let root = scratch_dir("deliver-cache-fail");
        // A regular file where the cache directory should be: creating or
        // renaming into it cannot succeed.
        let blocked = root.join("blocked");
        std::fs::write(&blocked, b"not a directory").expect("seed the blocker");
        let cache_path = blocked.join("chapter.mp3");
        let output_path = root.join("out/Chapter One.mp3");

        deliver_synthesized_export(b"paid-audio", &cache_path, &output_path)
            .await
            .expect("a cache failure must not deny the reader audio they paid for");

        assert_eq!(std::fs::read(&output_path).expect("output"), b"paid-audio");

        std::fs::remove_dir_all(&root).ok();
    }
}
