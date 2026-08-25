# Translation pre-release validation

Validated 2026-08-25 on an arm64 Mac Studio (Apple M1 Ultra, 128 GB) running
macOS 26.4. The validation harness is intentionally ignored by the normal test
suite because it needs about 314 MB of translation models, real imported
chapters, and (for the concurrency benchmark) the downloaded Supertonic model.

## Pinned model artifacts

The original `libretexts/opus-mt-*-ct2` repository names were not publicly
downloadable. The catalog now uses the published CTranslate2 conversions of
the Apache-2.0 Helsinki-NLP models, locks each repository to a 40-character
Hugging Face commit, and verifies every runtime file by byte length and SHA-256.
This retains the integrity property of the proposed organization-owned uploads:
a changed branch or file cannot be accepted. Availability still depends on the
community repositories remaining online.

| Pair | Repository revision | `model.bin` bytes | `model.bin` SHA-256 |
|---|---|---:|---|
| en → es | `michaelfeil/ct2fast-opus-mt-en-es@76ec296588e2234f9b7dfad5254219a0f5ecb7af` | 155,502,501 | `36cd9bcb181fc6d5832deeaf770ce183ff4edbbc5e4fe0f86cec92da4379f3b7` |
| es → en | `michaelfeil/ct2fast-opus-mt-es-en@437f5ffc6c8544943c685ea405650e0d17cf6098` | 155,502,501 | `3a3b91dcb396ee7b682554e7d9f501909385c48b478a691bfe9bf9e3e32d3656` |

The manifest also pins `config.json`, `shared_vocabulary.txt`, `source.spm`,
and `target.spm`. `catalog::tests::pinned_manifests_have_no_release_placeholders`
prevents a zero hash, zero byte length, moving `main` revision, or incomplete
manifest from becoming releasable again. The downloader resolves the pinned
revision rather than `main` and then performs its existing streaming SHA-256
verification.

Loading the real artifacts initially uncovered a process-aborting native symbol
collision: ct2rs's SentencePiece and the statically linked ONNX Runtime embedded
in Supertonic supplied incompatible protobuf versions. ct2rs's native tokenizer
feature is now disabled and the same `.spm` files are handled by the pure-Rust
`sentencepiece-rs` runtime. This removes the duplicate protobuf from the app and
allows translation and Supertonic to run concurrently.

## QA threshold calibration

The deterministic 5% sample was run through en → es → en for five real
chapters from three imported textbooks. A degraded pass rotated the Spanish
sentences by one position before back-translation, representing a catastrophic
alignment/configuration failure while holding the models and corpus constant.

| Chapter | Sentences / sampled | Healthy min / p10 / median | Degraded min / p10 / median |
|---|---:|---:|---:|
| General Biology — 2.1 Atoms, Isotopes, Ions, and Molecules | 317 / 16 | 32.75 / 79.43 / 86.67 | 2.96 / 7.83 / 17.77 |
| General Biology — 47.1 The Biodiversity Crisis | 276 / 14 | 32.75 / 66.57 / 83.62 | 3.30 / 5.10 / 22.29 |
| Foundations for Assisting in Home Care — Working with People with Physical Disabilities | 420 / 21 | 0.00 / 54.94 / 79.58 | 0.00 / 0.96 / 8.50 |
| Foundations for Assisting in Home Care — Family Spending and Budgeting | 440 / 22 | 24.68 / 49.48 / 88.82 | 0.00 / 0.00 / 3.97 |
| Fundamentals of Infrastructure Management — 5.2 Linear Optimization | 89 / 5 | 72.50 / 72.50 / 84.92 | 13.89 / 13.89 / 20.09 |

Healthy chapter p10 had a floor of 49.48 and degraded chapter p10 had a ceiling
of 13.89. Valid paraphrases scored as low as 24.68 (for example, “Amount
leftover” returning as “Surplus amount”), while a genuine mistranslation of the
label “FEEDBACK” scored 0. The configured threshold is therefore **20.0**: it
passes the observed valid paraphrases, catches the observed catastrophic output,
and remains inside the measured chapter-level separation. The previous 45.0
would have rejected several semantically valid results.

## Dense-chapter concurrency benchmark

The production batch shape (32 sentences) was measured on the 34,591-word
“Personal Care” chapter while Supertonic continuously synthesized Spanish text.
The chapter contains 2,824 sentences.

| Measurement | Result |
|---|---:|
| Forward translation | 80.97 s |
| Sample QA / no-escalation path (50 sentences) | 1.12 s |
| End-to-end no escalation | 82.09 s |
| Full reverse QA escalation, additional | 51.77 s |
| End-to-end escalated | 133.86 s |
| Progress updates | 90 |
| Longest batch/progress interval and cooperative-Cancel bound | 1.32 s |
| Supertonic Spanish utterances completed concurrently | 49 |
| QA failures at 20.0 | sample 0; full 18 |

The measured 1.32-second worst case is adequate for both visible progress and
cooperative Cancel: the app reports after every batch and observes cancellation
before starting the next one. Escalation is expensive but bounded; it adds about
52 seconds to this unusually large chapter while speech synthesis is active.

## Reproduce

Keep all downloaded artifacts and the copied database outside the repository.
Place the two verified manifests under `<model-root>/en-es` and
`<model-root>/es-en`, and supply an explicit copied database so ignored tests
never discover or modify the normal app-data database.

```bash
LIBRETEXTS_TRANSLATION_VALIDATION_DB=/tmp/ltr-validation/library.sqlite \
LIBRETEXTS_TRANSLATION_VALIDATION_MODEL_ROOT=/tmp/ltr-validation/models \
LIBRETEXTS_TRANSLATION_VALIDATION_SECTIONS=<comma-separated-section-ids> \
cargo test -p libretexts-reader pre_release_calibrates_translation_qa -- --ignored --nocapture

LIBRETEXTS_TRANSLATION_VALIDATION_DB=/tmp/ltr-validation/library.sqlite \
LIBRETEXTS_TRANSLATION_VALIDATION_MODEL_ROOT=/tmp/ltr-validation/models \
LIBRETEXTS_TRANSLATION_VALIDATION_SECTIONS=<dense-section-id> \
cargo test -p libretexts-reader pre_release_benchmarks_dense_chapter_with_supertonic -- --ignored --nocapture
```
