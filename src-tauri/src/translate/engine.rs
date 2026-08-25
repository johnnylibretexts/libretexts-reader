use std::path::Path;

use ctranslate2::{ComputeType, Config, Tokenizer, Translator};
use sentencepiece_rs::SentencePieceProcessor;

use crate::error::{AppError, AppResult};
use crate::translate::catalog::TranslationModel;
use crate::translate::mask::{mask_math, restore_math};
use crate::translate::provider::TranslationProvider;

/// `None` for any sentence whose math tokens did not survive. The caller
/// stores it as `rejected` and speaks the original.
pub(crate) fn translate_sentences(
    provider: &impl TranslationProvider,
    sentences: &[String],
) -> AppResult<Vec<Option<String>>> {
    let masked: Vec<_> = sentences
        .iter()
        .map(|sentence| mask_math(sentence))
        .collect();
    let payload: Vec<String> = masked.iter().map(|masked| masked.text.clone()).collect();
    let translated = provider.translate(&payload)?;
    Ok(translated
        .iter()
        .zip(masked.iter())
        .map(|(output, masked)| restore_math(output, &masked.tokens))
        .collect())
}

pub(crate) struct OpusMtEngine {
    translator: Translator<MarianTokenizer>,
}

/// Pure-Rust SentencePiece keeps its protobuf parser out of the native symbol
/// table. The native SentencePiece library embedded by ct2rs otherwise
/// collides with ONNX Runtime's newer protobuf and aborts the whole app during
/// model load.
struct MarianTokenizer {
    encoder: SentencePieceProcessor,
    decoder: SentencePieceProcessor,
}

impl MarianTokenizer {
    fn from_files(source: &Path, target: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            encoder: SentencePieceProcessor::open(source)?,
            decoder: SentencePieceProcessor::open(target)?,
        })
    }
}

impl Tokenizer for MarianTokenizer {
    fn encode(&self, input: &str) -> anyhow::Result<Vec<String>> {
        let mut pieces = self.encoder.encode(input)?;
        pieces.push("</s>".to_string());
        Ok(pieces)
    }

    fn decode(&self, tokens: Vec<String>) -> anyhow::Result<String> {
        self.decoder.decode(&tokens).map_err(Into::into)
    }
}

impl OpusMtEngine {
    pub(crate) fn load(model: &TranslationModel, root: &Path) -> AppResult<Self> {
        let tokenizer =
            MarianTokenizer::from_files(&root.join("source.spm"), &root.join("target.spm"))
                .map_err(|error| {
                    AppError::Model(format!(
                        "could not load tokenizer for {}: {error}",
                        model.model_id
                    ))
                })?;
        let config = Config {
            compute_type: ComputeType::INT8,
            ..Config::default()
        };
        let translator = Translator::with_tokenizer(root, tokenizer, &config).map_err(|error| {
            AppError::Model(format!(
                "could not load translation model {}: {error}",
                model.model_id
            ))
        })?;

        Ok(Self { translator })
    }
}

impl TranslationProvider for OpusMtEngine {
    fn translate(&self, sentences: &[String]) -> AppResult<Vec<String>> {
        self.translator
            .translate_batch(sentences, &Default::default(), None)
            .map(|results| results.into_iter().map(|(text, _score)| text).collect())
            .map_err(|error| AppError::Model(format!("translation failed: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use crate::error::AppResult;
    use crate::translate::provider::TranslationProvider;

    use super::translate_sentences;

    #[test]
    fn translating_masks_math_and_restores_it_or_falls_back() {
        // A fake provider: the trait exists so the pipeline is testable without a
        // 155MB model present, which is the whole reason it is a trait.
        struct EchoProvider;
        impl TranslationProvider for EchoProvider {
            fn translate(&self, sentences: &[String]) -> AppResult<Vec<String>> {
                Ok(sentences
                    .iter()
                    .map(|sentence| sentence.replace("Given", "Dado"))
                    .collect())
            }
        }
        struct LosesSentinels;
        impl TranslationProvider for LosesSentinels {
            fn translate(&self, sentences: &[String]) -> AppResult<Vec<String>> {
                Ok(sentences.iter().map(|_| "Dado.".to_string()).collect())
            }
        }

        let input = vec!["Given [[latex:eA==]].".to_string()];

        let good = translate_sentences(&EchoProvider, &input).unwrap();
        assert_eq!(good[0], Some("Dado [[latex:eA==]].".to_string()));

        let bad = translate_sentences(&LosesSentinels, &input).unwrap();
        assert_eq!(
            bad[0], None,
            "a lost equation must fall back, never be dropped silently"
        );
    }
}
