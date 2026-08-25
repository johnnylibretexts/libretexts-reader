# Translation pre-release validation

Validated 2026-08-25 on an arm64 Mac Studio (Apple M1 Ultra, 128 GB) running
macOS 26.4. The ignored validation harness uses copied real textbook data and
downloaded models outside the repository.

## Language coverage and pinned artifact

Supertonic 3 has 31 spoken languages. Its additional `na` value is a
language-agnostic pronunciation fallback, not a language. LibreTexts Reader
therefore supports 30 English-to-target and 30 target-to-English directions and
never offers `na` as a translation target.

All 60 directions use one bidirectional, MIT-licensed M2M100 CTranslate2 INT8
runtime. It downloads once, is shared when the reader changes languages, and is
not bundled in the app.

| Artifact | Value |
|---|---|
| Conversion | `gn64/M2M100_418M_CTranslate2` |
| Immutable revision | `18e406c615ef2991fa74d53734bf66b0a6b10cb4` |
| Upstream | `facebook/m2m100_418M` |
| Confirmed download | 495,887,877 bytes (~496 MB) |
| `model.bin` | 490,667,752 bytes; `a1826980fc5c037e69c7ac94fcb56c03001a66f380eb71863cc0a3879e71421b` |
| `config.json` | 223 bytes; `8f6496adfc930cbfecbe8281112197705c488fab47d34b4829b06d7f478909af` |
| `shared_vocabulary.json` | 2,796,509 bytes; `7eb5d0ff184c6095c7c10f9911c0aea492250abd12854f9c3d787c64b1c6397e` |
| `sentencepiece.bpe.model` | 2,423,393 bytes; `d8f7c76ed2a5e0822be39f0a4f95a55eb19c78f4593ce609e2edbc2aea4d380a` |

The downloader resolves the immutable revision and verifies every byte length
and SHA-256 before loading. A live Rust test downloaded the model into a fresh
temporary directory, verified it, translated English to Spanish, and preserved
a masked math token. The pure-Rust SentencePiece path remains necessary so
CTranslate2 does not introduce a native protobuf collision with Supertonic's
ONNX Runtime.

## QA threshold calibration

For every target, the deterministic 5% sample was run through English → target
→ English across five chapters from three textbooks: 317/16, 276/14, 420/21,
440/22, and 89/5 sentences/samples. A degraded pass rotated translated
sentences by one position before back-translation. That is 7,020 model inputs
across forward, healthy reverse, and degraded reverse passes.

The table records the highest degraded chapter p10, lowest healthy chapter p10,
and the configured midpoint threshold. Production uses the target-specific
threshold instead of applying Spanish's cutoff to every language.

| Target | Degraded ceiling | Healthy floor | Threshold |
|---|---:|---:|---:|
| Korean (`ko`) | 10.54 | 15.62 | 13.08 |
| Japanese (`ja`) | 9.98 | 17.86 | 13.92 |
| Arabic (`ar`) | 14.27 | 27.44 | 20.85 |
| Bulgarian (`bg`) | 15.92 | 22.73 | 19.32 |
| Czech (`cs`) | 14.09 | 28.42 | 21.25 |
| Danish (`da`) | 13.93 | 45.33 | 29.63 |
| German (`de`) | 14.36 | 31.61 | 22.98 |
| Greek (`el`) | 17.70 | 19.09 | 18.39 |
| Spanish (`es`) | 12.78 | 34.20 | 23.49 |
| Estonian (`et`) | 15.16 | 27.01 | 21.08 |
| Finnish (`fi`) | 7.68 | 20.97 | 14.32 |
| French (`fr`) | 14.63 | 17.86 | 16.25 |
| Hindi (`hi`) | 11.54 | 27.44 | 19.49 |
| Croatian (`hr`) | 18.48 | 36.82 | 27.65 |
| Hungarian (`hu`) | 12.59 | 31.70 | 22.14 |
| Indonesian (`id`) | 13.82 | 22.20 | 18.01 |
| Italian (`it`) | 14.63 | 33.55 | 24.09 |
| Lithuanian (`lt`) | 9.99 | 13.95 | 11.97 |
| Latvian (`lv`) | 13.13 | 29.60 | 21.37 |
| Dutch (`nl`) | 12.67 | 28.53 | 20.60 |
| Polish (`pl`) | 18.03 | 18.30 | 18.16 |
| Portuguese (`pt`) | 14.63 | 16.30 | 15.47 |
| Romanian (`ro`) | 14.63 | 29.92 | 22.28 |
| Russian (`ru`) | 10.15 | 36.01 | 23.08 |
| Slovak (`sk`) | 14.11 | 20.81 | 17.46 |
| Slovenian (`sl`) | 11.62 | 30.20 | 20.91 |
| Swedish (`sv`) | 14.55 | 36.82 | 25.68 |
| Turkish (`tr`) | 15.37 | 12.52 | 13.95 |
| Ukrainian (`uk`) | 14.31 | 22.21 | 18.26 |
| Vietnamese (`vi`) | 14.76 | 7.45 | 11.11 |

Twenty-eight targets produced a clean chapter-p10 separation. Polish's clean
band is narrow (0.27 points). Turkish and Vietnamese overlap, so their midpoint
is an explicit conservative compromise: some valid low-confidence sentences
can fall back to English, and some degraded sentences can survive the score.
The app never hides a rejected result; it speaks the source sentence and reports
the chapter's fallback count.

## Dense-chapter concurrency benchmark

The production batch shape was tuned on the 34,591-word “Personal Care” chapter
(2,824 sentences) while Supertonic completed 20 Spanish syntheses concurrently.
An initial 32-sentence run exposed two failures: full reverse QA sent the whole
chapter in one allocation, and the worst Cancel interval reached 20.34 seconds.
Reverse QA now uses bounded batches, and a ten-sentence production batch passed
the release gate.

| Measurement | Result |
|---|---:|
| Forward translation | 471.23 s |
| Sample QA / no-escalation path (50 sentences) | 6.99 s |
| End-to-end no escalation | 478.22 s |
| Full reverse QA escalation, additional | 415.53 s |
| End-to-end escalated | 893.75 s |
| Progress updates | 284 |
| Longest batch/progress and cooperative-Cancel interval | 3.94 s |
| Concurrent Supertonic Spanish utterances | 20 |
| Spanish QA failures at 23.49 | sample 1; full 54 |
| Observed bounded combined RSS | approximately 2.3–2.8 GB |

This is a deliberately pathological chapter. A 300–440 sentence chapter scales
to minutes rather than seconds, and completed translations are cached by
chapter, target, and immutable model ID. The ten-sentence batch trades some
throughput for a measured sub-four-second cancellation/progress bound.

## Packaged-app smoke test

The signed `0.1.0-beta.3` app was run directly from its release bundle on
2026-08-25 with an isolated copy of the library and model directory. The smoke
test exercised the real Tauri controls and persisted state rather than invoking
translation commands directly.

- Settings offered all 30 translated targets. Selecting Spanish showed the
  shared 496 MB download size, and the download action required a second
  confirmation naming English → Spanish and the storage cost.
- Download progress advanced in Settings. Cancel stopped the transfer at about
  100 MB and returned the model to Not downloaded; retry completed all four
  files, verified their pinned hashes, and changed the status to Ready.
- Playing the 98-character English Licensing section displayed the original
  English sentence throughout, translated it to Spanish with QA `passed`, zero
  fallbacks, and changed the playback control to Pause while Spanish narration
  was active.
- After quitting and relaunching the packaged app, Settings restored Spanish
  and revalidated the model as Ready. Replaying the same section used the
  existing translation immediately without changing its cache timestamp.
- Translated export produced a 145.38-second mono AAC M4A for the Preface. The
  filename ends in `- es.m4a`, the container metadata records `LANGUAGE=es`,
  and the export used 12 cached Spanish sentences from the pinned model with no
  QA failures or fallbacks.

The release `.app` passed strict deep code-signature verification, contained
all generated notices, and was 37 MB. A mounted validation DMG preserved the
inner signature and contained no bundled translation or Supertonic model files.

## Reproduce

Place the verified model files under
`<model-root>/m2m100-418m-int8-18e406c`, copy a real database to a temporary
location, and pass explicit section IDs so ignored tests never touch normal app
data.

```bash
LIBRETEXTS_TRANSLATION_VALIDATION_DB=/tmp/ltr-validation/library.sqlite \
LIBRETEXTS_TRANSLATION_VALIDATION_MODEL_ROOT=/tmp/ltr-validation-models \
LIBRETEXTS_TRANSLATION_VALIDATION_SECTIONS=<five-comma-separated-section-ids> \
cargo test -p libretexts-reader pre_release_calibrates_translation_qa -- --ignored --nocapture

LIBRETEXTS_TRANSLATION_VALIDATION_DB=/tmp/ltr-validation/library.sqlite \
LIBRETEXTS_TRANSLATION_VALIDATION_MODEL_ROOT=/tmp/ltr-validation-models \
LIBRETEXTS_TRANSLATION_VALIDATION_SECTIONS=<dense-section-id> \
LIBRETEXTS_TRANSLATION_BENCH_SYNTHESIS_LIMIT=20 \
cargo test -p libretexts-reader pre_release_benchmarks_dense_chapter_with_supertonic -- --ignored --nocapture
```
