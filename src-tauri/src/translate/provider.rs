use crate::error::AppResult;

/// Mirrors `TtsProvider`: the engine behind an interface so the pipeline can
/// be tested without a model on disk.
pub(crate) trait TranslationProvider {
    fn translate(&self, sentences: &[String]) -> AppResult<Vec<String>>;
}
