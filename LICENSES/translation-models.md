# The M2M100 translation model

The on-device translation model is **not bundled** with LibreTexts Reader.
After the reader selects a translated language and confirms the displayed
download size, the app downloads one shared model directly from Hugging Face
into the app-data directory. Nothing in the distributed `.app` contains its
weights.

## Terms and provenance

| Item | Source | Licence |
|---|---|---|
| CTranslate2 INT8 conversion | [`gn64/M2M100_418M_CTranslate2`](https://huggingface.co/gn64/M2M100_418M_CTranslate2) | MIT |
| Upstream weights | [`facebook/m2m100_418M`](https://huggingface.co/facebook/m2m100_418M) | MIT |

M2M100 supplies both directions between English and all 30 non-English
languages Supertonic 3 can pronounce. Supertonic's `na` value is a
language-agnostic pronunciation fallback, not a language, so the app never
offers it as a translation target.

The app locks the conversion to immutable commit
`18e406c615ef2991fa74d53734bf66b0a6b10cb4` and verifies the byte length and
SHA-256 of `model.bin`, `config.json`, `shared_vocabulary.json`, and
`sentencepiece.bpe.model` before loading them. The exact manifest is in
`src-tauri/src/translate/catalog.rs`; the language-by-language QA record is in
`docs/validation/translation-pre-release.md`.

## Distribution boundary

The download travels from Hugging Face to the reader's own machine after their
explicit confirmation. LibreTexts Reader distributes the downloader and the
CTranslate2 runtime, not the model weights. If the model is ever bundled in the
`.app` or mirrored by the project, revisit this file and include all notice and
source obligations that apply to that distribution.
