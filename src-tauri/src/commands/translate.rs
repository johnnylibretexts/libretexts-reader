use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use tauri::{Emitter, Runtime, State, Window};

use crate::db::connection::DbPool;
use crate::db::{library, settings};
use crate::error::{AppError, AppResult};
use crate::paths;
use crate::translate::catalog::{self, TranslationModel};
use crate::translate::engine::{translate_sentences, OpusMtEngine};
use crate::translate::mask::mask_math;
use crate::translate::model::{
    fetch_with_progress, model_status, TranslationDownload, TranslationDownloadCancel,
};
use crate::translate::provider::TranslationProvider;
use crate::translate::qa::{self, QA_THRESHOLD};

const TRANSLATION_BATCH_SIZE: usize = 32;

/// What the quality gate decided about one chapter.
///
/// `statuses` is parallel to the sentences passed in because persistence writes
/// each decision straight back to the matching sentence index.
pub(crate) struct QualityOutcome {
    pub statuses: Vec<&'static str>,
    pub escalated: bool,
    pub sampled: usize,
    pub failed: usize,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn run_quality_gate(
    sources: &[String],
    translations: &[String],
    reverse: &impl TranslationProvider,
) -> QualityOutcome {
    run_quality_gate_result(sources, translations, reverse).unwrap_or_else(|_| QualityOutcome {
        statuses: vec!["rejected"; sources.len()],
        escalated: true,
        sampled: qa::sample_indices(sources.len()).len(),
        failed: sources.len(),
    })
}

fn run_quality_gate_result(
    sources: &[String],
    translations: &[String],
    reverse: &impl TranslationProvider,
) -> AppResult<QualityOutcome> {
    let total = sources.len();
    if total == 0 {
        return Ok(QualityOutcome {
            statuses: Vec::new(),
            escalated: false,
            sampled: 0,
            failed: 0,
        });
    }
    let sample = qa::sample_indices(total);
    let mut statuses = vec!["unchecked"; total];

    let score = |indices: &[usize]| -> AppResult<Vec<f64>> {
        let targets = indices
            .iter()
            .map(|&index| {
                translations
                    .get(index)
                    .map(|text| mask_math(text).text)
                    .ok_or_else(|| {
                        AppError::Model(
                            "quality gate received misaligned translation results".into(),
                        )
                    })
            })
            .collect::<AppResult<Vec<_>>>()?;
        let back_translated = reverse.translate(&targets)?;
        if back_translated.len() != indices.len() {
            return Err(AppError::Model(format!(
                "back-translation returned {} sentences for {} inputs",
                back_translated.len(),
                indices.len()
            )));
        }
        Ok(indices
            .iter()
            .zip(back_translated)
            .map(|(&index, hypothesis)| {
                let reference = mask_math(&sources[index]).text;
                qa::chrf(&hypothesis, &reference)
            })
            .collect())
    };

    let sample_scores = score(&sample)?;

    for (&index, &score) in sample.iter().zip(&sample_scores) {
        statuses[index] = if score < QA_THRESHOLD {
            "rejected"
        } else {
            "passed"
        };
    }

    let escalated = qa::should_escalate(&sample_scores);
    if escalated {
        let all = (0..total).collect::<Vec<_>>();
        let scores = score(&all)?;
        for (status, score) in statuses.iter_mut().zip(scores) {
            *status = if score < QA_THRESHOLD {
                "rejected"
            } else {
                "passed"
            };
        }
    }

    let failed = statuses
        .iter()
        .filter(|status| **status == "rejected")
        .count();
    Ok(QualityOutcome {
        statuses,
        escalated,
        sampled: sample.len(),
        failed,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationModelStatus {
    pub downloaded: bool,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub verified: bool,
    pub missing_files: Vec<String>,
    pub source_lang: String,
    pub target_lang: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TranslateSectionResult {
    Original {
        source_lang: String,
        sentence_count: usize,
    },
    NeedsDownload {
        source_lang: String,
        target_lang: String,
        model_status: TranslationModelStatus,
    },
    Complete {
        source_lang: String,
        target_lang: String,
        fallback_count: usize,
        sentence_count: usize,
    },
    Cancelled {
        source_lang: String,
        target_lang: String,
        done: usize,
        total: usize,
        fallback_count: usize,
        sentence_count: usize,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranslationProgress {
    section_id: String,
    done: usize,
    total: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranslationModelDownloadProgress {
    source_lang: String,
    target_lang: String,
    pair: String,
    file: String,
    downloaded: u64,
    total: u64,
}

/// Cooperative cancellation for one chapter translation.
#[derive(Debug, Default, Clone)]
pub struct SectionTranslationCancel(Arc<AtomicBool>);

impl SectionTranslationCancel {
    fn request(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn clear(&self) {
        self.0.store(false, Ordering::SeqCst);
    }

    fn requested(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug)]
struct SentenceJob {
    paragraph_id: String,
    sentence_index: usize,
    source: String,
}

#[tauri::command]
pub async fn translate_section<R: Runtime>(
    window: Window<R>,
    state: State<'_, DbPool>,
    cancel: State<'_, SectionTranslationCancel>,
    section_id: String,
) -> AppResult<TranslateSectionResult> {
    let pool = state.inner().clone();
    let cancel = cancel.inner().clone();
    cancel.clear();
    tauri::async_runtime::spawn_blocking(move || {
        translate_section_inner(&window, &pool, &cancel, &section_id)
    })
    .await
    .map_err(|error| AppError::Model(format!("translation task stopped: {error}")))?
}

fn translate_section_inner<R: Runtime>(
    window: &Window<R>,
    pool: &DbPool,
    cancel: &SectionTranslationCancel,
    section_id: &str,
) -> AppResult<TranslateSectionResult> {
    let mut conn = pool.get()?;
    let source_lang = section_source_language(&conn, section_id)?;
    let jobs = collect_sentence_jobs(&conn, section_id)?;
    let sentence_count = jobs.len();
    let target_lang = saved_target_language(&conn)?;

    let Some(target_lang) = target_lang else {
        return Ok(TranslateSectionResult::Original {
            source_lang,
            sentence_count,
        });
    };
    if source_lang == target_lang {
        return Ok(TranslateSectionResult::Original {
            source_lang,
            sentence_count,
        });
    }

    let (forward, reverse) = pair_models(&source_lang, &target_lang)?;
    if !needs_translation(&conn, section_id, &target_lang, &forward.model_id)? {
        let fallback_count = conn.query_row(
            "SELECT fallback_count FROM section_translations
              WHERE section_id = ?1 AND target_lang = ?2",
            params![section_id, target_lang],
            |row| row.get(0),
        )?;
        return Ok(TranslateSectionResult::Complete {
            source_lang,
            target_lang,
            fallback_count,
            sentence_count,
        });
    }

    let models_root = paths::models_dir()?.join("translation");
    let status =
        combined_model_status(&source_lang, &target_lang, &forward, &reverse, &models_root);
    if !status.downloaded {
        return Ok(TranslateSectionResult::NeedsDownload {
            source_lang,
            target_lang,
            model_status: status,
        });
    }

    let forward_root = pair_model_root(&models_root, &source_lang, &target_lang);
    let reverse_root = pair_model_root(&models_root, &target_lang, &source_lang);
    let forward_engine = OpusMtEngine::load(&forward, &forward_root)?;
    let reverse_engine = OpusMtEngine::load(&reverse, &reverse_root)?;
    let mut translated = Vec::with_capacity(sentence_count);

    emit_translation_progress(window, section_id, 0, sentence_count)?;
    for batch in jobs.chunks(TRANSLATION_BATCH_SIZE) {
        if cancel.requested() {
            let fallback_count = persist_translation(
                &mut conn,
                section_id,
                &source_lang,
                &target_lang,
                &forward.model_id,
                &jobs,
                &translated,
                &partial_statuses(&translated),
                "running",
                0,
                false,
            )?;
            return Ok(TranslateSectionResult::Cancelled {
                source_lang,
                target_lang,
                done: translated.len(),
                total: sentence_count,
                fallback_count,
                sentence_count,
            });
        }

        let sources = batch
            .iter()
            .map(|sentence| sentence.source.clone())
            .collect::<Vec<_>>();
        let batch_output = match translate_sentences(&forward_engine, &sources) {
            Ok(output) if output.len() == batch.len() => output,
            Ok(output) => {
                let error = AppError::Model(format!(
                    "translation returned {} sentences for a batch of {}",
                    output.len(),
                    batch.len()
                ));
                persist_failed_translation(
                    &mut conn,
                    section_id,
                    &source_lang,
                    &target_lang,
                    &forward.model_id,
                    &jobs,
                    &translated,
                )?;
                return Err(error);
            }
            Err(error) => {
                persist_failed_translation(
                    &mut conn,
                    section_id,
                    &source_lang,
                    &target_lang,
                    &forward.model_id,
                    &jobs,
                    &translated,
                )?;
                return Err(error);
            }
        };
        translated.extend(batch_output);
        emit_translation_progress(window, section_id, translated.len(), sentence_count)?;
    }

    if cancel.requested() {
        let statuses = partial_statuses(&translated);
        let fallback_count = persist_translation(
            &mut conn,
            section_id,
            &source_lang,
            &target_lang,
            &forward.model_id,
            &jobs,
            &translated,
            &statuses,
            "running",
            0,
            false,
        )?;
        return Ok(TranslateSectionResult::Cancelled {
            source_lang,
            target_lang,
            done: translated.len(),
            total: sentence_count,
            fallback_count,
            sentence_count,
        });
    }

    let accepted = translated
        .iter()
        .enumerate()
        .filter_map(|(index, translation)| {
            translation
                .as_ref()
                .map(|translation| (index, jobs[index].source.clone(), translation.clone()))
        })
        .collect::<Vec<_>>();
    let quality = match run_quality_gate_result(
        &accepted
            .iter()
            .map(|(_, source, _)| source.clone())
            .collect::<Vec<_>>(),
        &accepted
            .iter()
            .map(|(_, _, translation)| translation.clone())
            .collect::<Vec<_>>(),
        &reverse_engine,
    ) {
        Ok(quality) => quality,
        Err(error) => {
            persist_failed_translation(
                &mut conn,
                section_id,
                &source_lang,
                &target_lang,
                &forward.model_id,
                &jobs,
                &translated,
            )?;
            return Err(error);
        }
    };
    let mut statuses = partial_statuses(&translated);
    for ((index, _, _), status) in accepted.iter().zip(&quality.statuses) {
        statuses[*index] = status;
    }
    debug_assert_eq!(
        quality.failed,
        quality
            .statuses
            .iter()
            .filter(|status| **status == "rejected")
            .count()
    );
    let fallback_count = persist_translation(
        &mut conn,
        section_id,
        &source_lang,
        &target_lang,
        &forward.model_id,
        &jobs,
        &translated,
        &statuses,
        "complete",
        quality.sampled,
        quality.escalated,
    )?;

    Ok(TranslateSectionResult::Complete {
        source_lang,
        target_lang,
        fallback_count,
        sentence_count,
    })
}

#[tauri::command]
pub async fn cancel_section_translation(
    cancel: State<'_, SectionTranslationCancel>,
) -> AppResult<()> {
    cancel.request();
    Ok(())
}

#[tauri::command]
pub async fn get_translation_model_status(
    source_lang: String,
    target_lang: String,
) -> AppResult<TranslationModelStatus> {
    let (forward, reverse) = pair_models(&source_lang, &target_lang)?;
    let root = paths::models_dir()?.join("translation");
    Ok(combined_model_status(
        &source_lang,
        &target_lang,
        &forward,
        &reverse,
        &root,
    ))
}

#[tauri::command]
pub async fn ensure_translation_models_downloaded<R: Runtime>(
    window: Window<R>,
    download: State<'_, TranslationDownload>,
    source_lang: String,
    target_lang: String,
) -> AppResult<String> {
    let (forward, reverse) = pair_models(&source_lang, &target_lang)?;
    let models_root = paths::models_dir()?.join("translation");
    let download = download.inner().clone();

    download
        .run(|cancel| async move {
            fetch_pair_with_progress(
                &window,
                &models_root,
                &source_lang,
                &target_lang,
                &forward,
                &reverse,
                cancel,
            )
            .await
        })
        .await
}

#[tauri::command]
pub async fn cancel_translation_model_download(
    download: State<'_, TranslationDownload>,
) -> AppResult<()> {
    download.request_cancel();
    Ok(())
}

#[tauri::command]
pub async fn list_translation_targets(source_lang: Option<String>) -> AppResult<Vec<String>> {
    let source_lang = source_lang.as_deref().unwrap_or("en");
    Ok(catalog::available_targets(source_lang)
        .into_iter()
        .filter(|target| catalog::resolve_pair(target, source_lang).is_some())
        .collect())
}

async fn fetch_pair_with_progress<R: Runtime>(
    window: &Window<R>,
    models_root: &Path,
    source_lang: &str,
    target_lang: &str,
    forward: &TranslationModel,
    reverse: &TranslationModel,
    cancel: TranslationDownloadCancel,
) -> AppResult<String> {
    let forward_total = forward
        .files
        .iter()
        .map(|file| file.size_bytes)
        .sum::<u64>();
    let reverse_total = reverse
        .files
        .iter()
        .map(|file| file.size_bytes)
        .sum::<u64>();
    let total = forward_total + reverse_total;
    let forward_root = pair_model_root(models_root, source_lang, target_lang);
    let reverse_root = pair_model_root(models_root, target_lang, source_lang);

    fetch_with_progress(forward, &forward_root, cancel.clone(), |file, done, _| {
        emit_model_download_progress(
            window,
            source_lang,
            target_lang,
            &format!("{source_lang}-{target_lang}"),
            file,
            done,
            total.max(done),
        )
    })
    .await?;

    let forward_downloaded = model_status(forward, &forward_root).downloaded_bytes;

    fetch_with_progress(reverse, &reverse_root, cancel, |file, done, _| {
        let combined = forward_downloaded + done;
        emit_model_download_progress(
            window,
            source_lang,
            target_lang,
            &format!("{target_lang}-{source_lang}"),
            file,
            combined,
            total.max(combined),
        )
    })
    .await?;

    let status = combined_model_status(source_lang, target_lang, forward, reverse, models_root);
    if !status.downloaded {
        return Err(AppError::Model(
            "translation model download is incomplete".into(),
        ));
    }
    Ok(models_root.to_string_lossy().into_owned())
}

fn emit_translation_progress<R: Runtime>(
    window: &Window<R>,
    section_id: &str,
    done: usize,
    total: usize,
) -> AppResult<()> {
    window.emit(
        "translation-progress",
        TranslationProgress {
            section_id: section_id.to_string(),
            done,
            total,
        },
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_model_download_progress<R: Runtime>(
    window: &Window<R>,
    source_lang: &str,
    target_lang: &str,
    pair: &str,
    file: &str,
    downloaded: u64,
    total: u64,
) -> AppResult<()> {
    window.emit(
        "translation-model-download-progress",
        TranslationModelDownloadProgress {
            source_lang: source_lang.to_string(),
            target_lang: target_lang.to_string(),
            pair: pair.to_string(),
            file: file.to_string(),
            downloaded,
            total,
        },
    )?;
    Ok(())
}

fn pair_models(
    source_lang: &str,
    target_lang: &str,
) -> AppResult<(TranslationModel, TranslationModel)> {
    let unavailable = || {
        AppError::Model(format!(
            "No on-device translation models are available for {source_lang} → {target_lang}. Choose Original language to read this book without translation."
        ))
    };
    let forward = catalog::resolve_pair(source_lang, target_lang).ok_or_else(unavailable)?;
    let reverse = catalog::resolve_pair(target_lang, source_lang).ok_or_else(unavailable)?;
    Ok((forward, reverse))
}

fn pair_model_root(models_root: &Path, source_lang: &str, target_lang: &str) -> PathBuf {
    models_root.join(format!("{source_lang}-{target_lang}"))
}

fn combined_model_status(
    source_lang: &str,
    target_lang: &str,
    forward: &TranslationModel,
    reverse: &TranslationModel,
    models_root: &Path,
) -> TranslationModelStatus {
    let forward_status = model_status(
        forward,
        &pair_model_root(models_root, source_lang, target_lang),
    );
    let reverse_status = model_status(
        reverse,
        &pair_model_root(models_root, target_lang, source_lang),
    );
    let mut missing_files = forward_status
        .missing_files
        .into_iter()
        .map(|file| format!("{source_lang}-{target_lang}/{file}"))
        .collect::<Vec<_>>();
    missing_files.extend(
        reverse_status
            .missing_files
            .into_iter()
            .map(|file| format!("{target_lang}-{source_lang}/{file}")),
    );

    TranslationModelStatus {
        downloaded: forward_status.downloaded && reverse_status.downloaded,
        downloaded_bytes: forward_status.downloaded_bytes + reverse_status.downloaded_bytes,
        total_bytes: forward_status.total_bytes + reverse_status.total_bytes,
        verified: forward.verified && reverse.verified,
        missing_files,
        source_lang: source_lang.to_string(),
        target_lang: target_lang.to_string(),
    }
}

fn section_source_language(conn: &Connection, section_id: &str) -> AppResult<String> {
    conn.query_row(
        "SELECT COALESCE(NULLIF(TRIM(d.source_language), ''), 'en')
           FROM sections s
           JOIN documents d ON d.id = s.document_id
          WHERE s.id = ?1",
        [section_id],
        |row| row.get::<_, String>(0),
    )
    .map(|language| language.to_lowercase())
    .map_err(Into::into)
}

fn saved_target_language(conn: &Connection) -> AppResult<Option<String>> {
    Ok(settings::get_setting(conn, "translation_target_lang")?
        .and_then(|value| value.as_str().map(str::trim).map(str::to_lowercase))
        .filter(|value| !value.is_empty() && value != "original"))
}

fn collect_sentence_jobs(conn: &Connection, section_id: &str) -> AppResult<Vec<SentenceJob>> {
    let paragraphs = library::list_paragraphs(conn, section_id, None)?;
    let mut jobs = Vec::new();
    for paragraph in paragraphs {
        for (sentence_index, (start, end)) in paragraph.sentence_offsets.iter().enumerate() {
            let source = paragraph.text.get(*start..*end).ok_or_else(|| {
                AppError::InvalidInput(format!(
                    "paragraph {} has an invalid sentence offset",
                    paragraph.id
                ))
            })?;
            jobs.push(SentenceJob {
                paragraph_id: paragraph.id.clone(),
                sentence_index,
                source: source.to_string(),
            });
        }
    }
    Ok(jobs)
}

fn partial_statuses(translated: &[Option<String>]) -> Vec<&'static str> {
    translated
        .iter()
        .map(|translation| {
            if translation.is_some() {
                "unchecked"
            } else {
                "rejected"
            }
        })
        .collect()
}

fn persist_failed_translation(
    conn: &mut Connection,
    section_id: &str,
    source_lang: &str,
    target_lang: &str,
    model_id: &str,
    jobs: &[SentenceJob],
    translated: &[Option<String>],
) -> AppResult<()> {
    let statuses = partial_statuses(translated);
    persist_translation(
        conn,
        section_id,
        source_lang,
        target_lang,
        model_id,
        jobs,
        translated,
        &statuses,
        "failed",
        0,
        false,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn persist_translation(
    conn: &mut Connection,
    section_id: &str,
    source_lang: &str,
    target_lang: &str,
    model_id: &str,
    jobs: &[SentenceJob],
    translated: &[Option<String>],
    statuses: &[&str],
    status: &str,
    qa_sampled: usize,
    qa_escalated: bool,
) -> AppResult<usize> {
    if translated.len() != statuses.len() || translated.len() > jobs.len() {
        return Err(AppError::Model(
            "translation persistence received misaligned sentence results".into(),
        ));
    }
    let fallback_count = jobs.len()
        - translated
            .iter()
            .zip(statuses)
            .filter(|(translation, status)| translation.is_some() && **status != "rejected")
            .count();
    let qa_failed = statuses
        .iter()
        .filter(|status| **status == "rejected")
        .count();
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM sentence_translations
          WHERE target_lang = ?1
            AND paragraph_id IN (SELECT id FROM paragraphs WHERE section_id = ?2)",
        params![target_lang, section_id],
    )?;
    for ((job, translation), qa_status) in jobs.iter().zip(translated).zip(statuses) {
        tx.execute(
            "INSERT INTO sentence_translations
                 (paragraph_id, sentence_index, target_lang, text, qa_status)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                job.paragraph_id,
                job.sentence_index as i64,
                target_lang,
                translation.as_deref().unwrap_or(&job.source),
                qa_status,
            ],
        )?;
    }
    tx.execute(
        "INSERT INTO section_translations
             (section_id, target_lang, source_lang, status, model_id,
              qa_sampled, qa_failed, qa_escalated, fallback_count, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(section_id, target_lang) DO UPDATE SET
             source_lang = excluded.source_lang,
             status = excluded.status,
             model_id = excluded.model_id,
             qa_sampled = excluded.qa_sampled,
             qa_failed = excluded.qa_failed,
             qa_escalated = excluded.qa_escalated,
             fallback_count = excluded.fallback_count,
             updated_at = excluded.updated_at",
        params![
            section_id,
            target_lang,
            source_lang,
            status,
            model_id,
            qa_sampled as i64,
            qa_failed as i64,
            i64::from(qa_escalated),
            fallback_count as i64,
            chrono::Utc::now().to_rfc3339(),
        ],
    )?;
    tx.commit()?;
    Ok(fallback_count)
}

pub(crate) fn needs_translation(
    conn: &Connection,
    section_id: &str,
    target_lang: &str,
    model_id: &str,
) -> AppResult<bool> {
    let current = conn
        .query_row(
            "SELECT status, model_id FROM section_translations
              WHERE section_id = ?1 AND target_lang = ?2",
            params![section_id, target_lang],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    Ok(!matches!(
        current,
        Some((status, stored_model)) if status == "complete" && stored_model == model_id
    ))
}

#[cfg(test)]
mod tests {
    use crate::db::migrations::apply_migrations;

    use super::*;

    struct FakeReverse;

    impl TranslationProvider for FakeReverse {
        fn translate(&self, sentences: &[String]) -> AppResult<Vec<String>> {
            Ok(sentences
                .iter()
                .map(|sentence| {
                    if sentence.contains("zzz") {
                        "qqq qqq qqq".to_string()
                    } else {
                        "The cell divides.".to_string()
                    }
                })
                .collect())
        }
    }

    #[test]
    fn a_failing_sample_escalates_and_rejects_only_the_bad_sentences() {
        let outcome = run_quality_gate(
            &[
                "The cell divides.".to_string(),
                "Mitosis begins.".to_string(),
            ],
            &["La célula se divide.".to_string(), "zzz".to_string()],
            &FakeReverse,
        );
        assert!(outcome.escalated);
        assert_eq!(outcome.statuses[0], "passed");
        assert_eq!(outcome.statuses[1], "rejected");
        assert_eq!(outcome.failed, 1);
    }

    #[test]
    fn a_healthy_chapter_is_not_escalated_and_leaves_unsampled_sentences_unchecked() {
        struct FaithfulReverse;
        impl TranslationProvider for FaithfulReverse {
            fn translate(&self, sentences: &[String]) -> AppResult<Vec<String>> {
                Ok(sentences
                    .iter()
                    .map(|_| "The cell divides.".to_string())
                    .collect())
            }
        }

        let sources = vec!["The cell divides.".to_string(); 100];
        let translations = vec!["La célula se divide.".to_string(); 100];
        let outcome = run_quality_gate(&sources, &translations, &FaithfulReverse);

        assert!(!outcome.escalated);
        assert_eq!(outcome.sampled, 5, "5% of 100");
        assert_eq!(
            outcome
                .statuses
                .iter()
                .filter(|status| **status == "unchecked")
                .count(),
            95
        );
    }

    fn seed_translated_section(model_id: &str) -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_migrations(&mut conn).unwrap();
        conn.execute_batch(
            "INSERT INTO documents
                 (id, title, source_type, source_metadata, word_count, imported_at)
             VALUES ('doc-1', 'A Book', 'openstax', '{}', 8, '2026-01-01T00:00:00Z');
             INSERT INTO sections (id, document_id, ordinal, title, word_count)
             VALUES ('sec-1', 'doc-1', 0, 'Chapter One', 8);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO section_translations
                 (section_id, target_lang, source_lang, status, model_id, updated_at)
             VALUES ('sec-1', 'es', 'en', 'complete', ?1, '2026-01-01T00:00:00Z')",
            [model_id],
        )
        .unwrap();
        conn
    }

    #[test]
    fn a_chapter_translated_by_a_different_model_is_retranslated() {
        let conn = seed_translated_section("libretexts/opus-mt-en-es-ct2@v1");
        assert!(
            needs_translation(&conn, "sec-1", "es", "libretexts/opus-mt-en-es-ct2@v2").unwrap()
        );
        assert!(
            !needs_translation(&conn, "sec-1", "es", "libretexts/opus-mt-en-es-ct2@v1").unwrap()
        );
    }

    #[test]
    fn a_chapter_never_translated_or_left_incomplete_needs_translating() {
        let conn = seed_translated_section("m1");
        assert!(needs_translation(&conn, "sec-1", "fr", "m1").unwrap());

        conn.execute(
            "UPDATE section_translations SET status = 'running' WHERE section_id = 'sec-1'",
            [],
        )
        .unwrap();
        assert!(needs_translation(&conn, "sec-1", "es", "m1").unwrap());
    }

    #[test]
    fn a_partial_retranslation_replaces_every_old_model_sentence_atomically() {
        let mut conn = seed_translated_section("old-model");
        conn.execute_batch(
            "INSERT INTO paragraphs (id, section_id, ordinal, text, sentence_offsets)
             VALUES ('p-1', 'sec-1', 0, 'One. Two.', '[[0,4],[5,9]]');
             INSERT INTO sentence_translations
                 (paragraph_id, sentence_index, target_lang, text, qa_status)
             VALUES ('p-1', 0, 'es', 'Viejo uno.', 'passed'),
                    ('p-1', 1, 'es', 'Viejo dos.', 'passed');",
        )
        .unwrap();
        let jobs = vec![
            SentenceJob {
                paragraph_id: "p-1".into(),
                sentence_index: 0,
                source: "One.".into(),
            },
            SentenceJob {
                paragraph_id: "p-1".into(),
                sentence_index: 1,
                source: "Two.".into(),
            },
        ];

        let fallback_count = persist_translation(
            &mut conn,
            "sec-1",
            "en",
            "es",
            "new-model",
            &jobs,
            &[Some("Nuevo uno.".into())],
            &["unchecked"],
            "running",
            0,
            false,
        )
        .unwrap();

        assert_eq!(fallback_count, 1);
        let rows: Vec<(i64, String)> = {
            let mut statement = conn
                .prepare(
                    "SELECT sentence_index, text FROM sentence_translations
                      WHERE paragraph_id = 'p-1' AND target_lang = 'es'
                      ORDER BY sentence_index",
                )
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(rows, [(0, "Nuevo uno.".to_string())]);
        let state: (String, String, i64) = conn
            .query_row(
                "SELECT status, model_id, fallback_count FROM section_translations
                  WHERE section_id = 'sec-1' AND target_lang = 'es'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(state, ("running".into(), "new-model".into(), 1));
    }

    #[test]
    fn model_status_combines_the_forward_and_reverse_downloads_in_a_temp_root() {
        let root = tempfile::tempdir().unwrap();
        let (forward, reverse) = pair_models("en", "es").unwrap();
        let status = combined_model_status("en", "es", &forward, &reverse, root.path());

        assert!(!status.downloaded);
        assert_eq!(
            status.total_bytes,
            forward
                .files
                .iter()
                .map(|file| file.size_bytes)
                .sum::<u64>()
                + reverse
                    .files
                    .iter()
                    .map(|file| file.size_bytes)
                    .sum::<u64>()
        );
        assert!(status.verified);
        assert!(status
            .missing_files
            .iter()
            .any(|file| file == "en-es/model.bin"));
        assert!(status
            .missing_files
            .iter()
            .any(|file| file == "es-en/model.bin"));
    }

    #[tokio::test]
    async fn target_list_requires_the_reverse_model_used_for_quality_checks() {
        let targets = list_translation_targets(Some("en".into())).await.unwrap();
        assert!(targets.contains(&"es".to_string()));
        assert!(
            !targets.contains(&"sw".to_string()),
            "the catalogue has en→sw but no sw→en QA model"
        );
    }

    #[test]
    fn command_results_serialize_with_the_frontend_field_names() {
        let value = serde_json::to_value(TranslateSectionResult::Complete {
            source_lang: "en".into(),
            target_lang: "es".into(),
            fallback_count: 2,
            sentence_count: 10,
        })
        .unwrap();

        assert_eq!(value["status"], "complete");
        assert_eq!(value["sourceLang"], "en");
        assert_eq!(value["fallbackCount"], 2);
        assert!(value.get("source_lang").is_none());
    }

    #[tokio::test]
    #[ignore = "downloads a real model and translates over the network"]
    async fn live_translates_small_chapter_en_es() {
        let root = tempfile::tempdir().unwrap();
        let model = crate::translate::catalog::resolve_pair("en", "es").unwrap();
        let download = crate::translate::model::TranslationDownload::default();
        download
            .run(|cancel| crate::translate::model::fetch(&model, root.path(), cancel))
            .await
            .expect("model download");

        let engine = crate::translate::engine::OpusMtEngine::load(&model, root.path()).unwrap();
        let sentences = vec![
            "The cell divides into two daughter cells.".to_string(),
            "Solve [[latex:eA==]] for x.".to_string(),
        ];
        let out = crate::translate::engine::translate_sentences(&engine, &sentences).unwrap();

        let plain = out[0].as_ref().expect("plain sentence must translate");
        assert_ne!(plain, &sentences[0], "output is still English");
        assert!(plain.to_lowercase().contains("célula"), "got: {plain}");
        if let Some(with_math) = &out[1] {
            assert!(with_math.contains("[[latex:eA==]]"), "got: {with_math}");
        }
    }

    fn validation_setting(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| {
            panic!("{name} is required; see docs/validation/translation-pre-release.md")
        })
    }

    fn validation_sections() -> Vec<String> {
        validation_setting("LIBRETEXTS_TRANSLATION_VALIDATION_SECTIONS")
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn validation_engines() -> (OpusMtEngine, OpusMtEngine) {
        let root = PathBuf::from(validation_setting(
            "LIBRETEXTS_TRANSLATION_VALIDATION_MODEL_ROOT",
        ));
        let (forward, reverse) = pair_models("en", "es").expect("en-es model pair");
        let status = combined_model_status("en", "es", &forward, &reverse, &root);
        assert!(
            status.downloaded,
            "validation models are incomplete: {:?}",
            status.missing_files
        );
        (
            OpusMtEngine::load(&forward, &pair_model_root(&root, "en", "es"))
                .expect("load en-es model"),
            OpusMtEngine::load(&reverse, &pair_model_root(&root, "es", "en"))
                .expect("load es-en model"),
        )
    }

    fn validation_connection() -> Connection {
        Connection::open(validation_setting("LIBRETEXTS_TRANSLATION_VALIDATION_DB"))
            .expect("open the explicit validation database")
    }

    fn score_back_translations(
        sources: &[String],
        translations: &[String],
        reverse: &OpusMtEngine,
    ) -> Vec<f64> {
        let back = back_translate(translations, reverse);
        sources
            .iter()
            .zip(back)
            .map(|(source, hypothesis)| qa::chrf(&hypothesis, &mask_math(source).text))
            .collect()
    }

    fn back_translate(translations: &[String], reverse: &OpusMtEngine) -> Vec<String> {
        let targets = translations
            .iter()
            .map(|translation| mask_math(translation).text)
            .collect::<Vec<_>>();
        reverse.translate(&targets).expect("back-translation")
    }

    fn describe_scores(scores: &[f64]) -> (f64, f64, f64, f64) {
        let mut sorted = scores.to_vec();
        sorted.sort_by(f64::total_cmp);
        let at = |fraction: f64| {
            let index = ((sorted.len() - 1) as f64 * fraction).round() as usize;
            sorted[index]
        };
        let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
        (sorted[0], at(0.10), at(0.50), mean)
    }

    /// Empirical QA calibration using deterministic 5% samples from multiple
    /// real chapters. The degraded pass deliberately assigns each Spanish
    /// sentence to the next English source sentence, modelling a catastrophic
    /// alignment/configuration failure without introducing another model.
    #[test]
    #[ignore = "requires explicit real chapters and downloaded translation models"]
    fn pre_release_calibrates_translation_qa() {
        let conn = validation_connection();
        let (forward, reverse) = validation_engines();
        let mut healthy_chapter_p10 = Vec::new();
        let mut degraded_chapter_p10 = Vec::new();

        for section_id in validation_sections() {
            let (document, section): (String, String) = conn
                .query_row(
                    "SELECT d.title, s.title FROM sections s
                      JOIN documents d ON d.id = s.document_id
                     WHERE s.id = ?1",
                    [&section_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("validation section exists");
            let jobs = collect_sentence_jobs(&conn, &section_id).expect("collect chapter text");
            let indices = qa::sample_indices(jobs.len());
            let sources = indices
                .iter()
                .map(|&index| jobs[index].source.clone())
                .collect::<Vec<_>>();
            let translated = translate_sentences(&forward, &sources)
                .expect("forward translation")
                .into_iter()
                .enumerate()
                .map(|(index, translated)| {
                    translated.unwrap_or_else(|| {
                        panic!("math masking rejected validation sample {index}")
                    })
                })
                .collect::<Vec<_>>();

            let healthy_back = back_translate(&translated, &reverse);
            let healthy = sources
                .iter()
                .zip(&healthy_back)
                .map(|(source, hypothesis)| qa::chrf(hypothesis, &mask_math(source).text))
                .collect::<Vec<_>>();
            let mut degraded_translations = translated.clone();
            degraded_translations.rotate_left(1);
            let degraded = score_back_translations(&sources, &degraded_translations, &reverse);
            let healthy_stats = describe_scores(&healthy);
            let degraded_stats = describe_scores(&degraded);
            healthy_chapter_p10.push(healthy_stats.1);
            degraded_chapter_p10.push(degraded_stats.1);

            println!(
                "{document} — {section} ({} sentences, {} sampled)\n  healthy  min={:.2} p10={:.2} median={:.2} mean={:.2}\n  degraded min={:.2} p10={:.2} median={:.2} mean={:.2}",
                jobs.len(),
                sources.len(),
                healthy_stats.0,
                healthy_stats.1,
                healthy_stats.2,
                healthy_stats.3,
                degraded_stats.0,
                degraded_stats.1,
                degraded_stats.2,
                degraded_stats.3,
            );
            for ((source, translation), (back, score)) in sources
                .iter()
                .zip(&translated)
                .zip(healthy_back.iter().zip(&healthy))
                .filter(|(_, (_, score))| **score < QA_THRESHOLD)
            {
                println!(
                    "  below threshold ({score:.2})\n    source: {source:?}\n    translated: {translation:?}\n    back: {back:?}"
                );
            }
        }

        assert!(
            healthy_chapter_p10.len() >= 3,
            "calibration requires at least three real chapters"
        );
        let healthy_floor = healthy_chapter_p10
            .into_iter()
            .reduce(f64::min)
            .expect("healthy scores");
        let degraded_ceiling = degraded_chapter_p10
            .into_iter()
            .reduce(f64::max)
            .expect("degraded scores");
        assert!(
            healthy_floor > degraded_ceiling,
            "no clean threshold separates healthy chapter p10 ({healthy_floor:.2}) from degraded chapter p10 ({degraded_ceiling:.2})"
        );
        let recommended = (healthy_floor + degraded_ceiling) / 2.0;
        println!(
            "chapter-p10 separation: degraded ceiling={degraded_ceiling:.2}, healthy floor={healthy_floor:.2}; midpoint={recommended:.2}, configured threshold={QA_THRESHOLD:.2}"
        );
        assert!(QA_THRESHOLD > degraded_ceiling && QA_THRESHOLD < healthy_floor);
    }

    /// Measures the production batch shape on one dense chapter. The sample
    /// path is the no-escalation cost; the full reverse pass is the additional
    /// escalation cost. Supertonic runs concurrently to expose CPU contention.
    #[test]
    #[ignore = "requires explicit real chapter, translation models, and Supertonic model"]
    fn pre_release_benchmarks_dense_chapter_with_supertonic() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        let conn = validation_connection();
        let section_id = validation_sections()
            .into_iter()
            .next()
            .expect("one dense validation section");
        let jobs = collect_sentence_jobs(&conn, &section_id).expect("collect dense chapter text");
        let sources = jobs
            .iter()
            .map(|job| job.source.clone())
            .collect::<Vec<_>>();
        let (forward, reverse) = validation_engines();

        let stop = Arc::new(AtomicBool::new(false));
        let synthesis_count = Arc::new(AtomicUsize::new(0));
        let synthesis_stop = stop.clone();
        let synthesis_total = synthesis_count.clone();
        let synthesis = std::thread::spawn(move || {
            while !synthesis_stop.load(Ordering::Relaxed) {
                crate::tts::supertonic::engine::synthesize_samples_blocking(
                    "Las células utilizan energía para mantener su organización, crecer y reproducirse.",
                    "M1",
                    "es",
                    1.0,
                )
                .expect("concurrent Supertonic synthesis");
                synthesis_total.fetch_add(1, Ordering::Relaxed);
            }
        });

        let forward_started = Instant::now();
        let mut longest_progress_interval = Duration::ZERO;
        let mut translated = Vec::with_capacity(sources.len());
        for batch in sources.chunks(TRANSLATION_BATCH_SIZE) {
            let batch_started = Instant::now();
            let output = translate_sentences(&forward, batch).expect("forward batch");
            longest_progress_interval = longest_progress_interval.max(batch_started.elapsed());
            translated.extend(
                output
                    .into_iter()
                    .zip(batch)
                    .map(|(translation, source)| translation.unwrap_or_else(|| source.clone())),
            );
        }
        let forward_elapsed = forward_started.elapsed();

        let sample_indices = qa::sample_indices(sources.len());
        let sample_sources = sample_indices
            .iter()
            .map(|&index| sources[index].clone())
            .collect::<Vec<_>>();
        let sample_translations = sample_indices
            .iter()
            .map(|&index| translated[index].clone())
            .collect::<Vec<_>>();
        let sample_started = Instant::now();
        let sample_scores =
            score_back_translations(&sample_sources, &sample_translations, &reverse);
        let sample_elapsed = sample_started.elapsed();

        let escalation_started = Instant::now();
        let all_scores = score_back_translations(&sources, &translated, &reverse);
        let escalation_elapsed = escalation_started.elapsed();
        stop.store(true, Ordering::Relaxed);
        synthesis.join().expect("Supertonic worker");

        println!(
            "dense chapter: {} sentences\nforward: {:.2}s\nQA sample/no escalation: {:.2}s ({} sentences)\nQA full escalation: +{:.2}s\nend-to-end no escalation: {:.2}s\nend-to-end escalated: {:.2}s\nprogress updates: {}; longest interval/cancel bound: {:.2}s\nSupertonic utterances completed concurrently: {}\nQA failures at {:.1}: sample={}, full={}",
            sources.len(),
            forward_elapsed.as_secs_f64(),
            sample_elapsed.as_secs_f64(),
            sample_indices.len(),
            escalation_elapsed.as_secs_f64(),
            (forward_elapsed + sample_elapsed).as_secs_f64(),
            (forward_elapsed + sample_elapsed + escalation_elapsed).as_secs_f64(),
            sources.len().div_ceil(TRANSLATION_BATCH_SIZE) + 1,
            longest_progress_interval.as_secs_f64(),
            synthesis_count.load(Ordering::Relaxed),
            QA_THRESHOLD,
            sample_scores.iter().filter(|score| **score < QA_THRESHOLD).count(),
            all_scores.iter().filter(|score| **score < QA_THRESHOLD).count(),
        );

        assert!(
            longest_progress_interval < Duration::from_secs(10),
            "progress and cooperative Cancel can be silent for {:.2}s",
            longest_progress_interval.as_secs_f64()
        );
    }
}
