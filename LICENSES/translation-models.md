# The Opus-MT translation models

The on-device translation models are **not bundled** with LibreTexts Reader.
After the reader selects a translated language and confirms the displayed
download size, the app downloads the forward and reverse model files directly
from Hugging Face into the app-data directory. Nothing in the distributed
`.app` contains these weights.

## Terms and provenance

| Direction | Conversion | Upstream weights | Licence |
|---|---|---|---|
| English → Spanish | [`michaelfeil/ct2fast-opus-mt-en-es`](https://huggingface.co/michaelfeil/ct2fast-opus-mt-en-es) | [`Helsinki-NLP/opus-mt-en-es`](https://huggingface.co/Helsinki-NLP/opus-mt-en-es) | Apache-2.0 |
| Spanish → English | [`michaelfeil/ct2fast-opus-mt-es-en`](https://huggingface.co/michaelfeil/ct2fast-opus-mt-es-en) | [`Helsinki-NLP/opus-mt-es-en`](https://huggingface.co/Helsinki-NLP/opus-mt-es-en) | Apache-2.0 |

The conversion repositories identify themselves as quantized CTranslate2
versions of the Helsinki-NLP models and retain the upstream Apache-2.0 terms.
The app locks each conversion to an immutable repository commit and verifies
the size and SHA-256 of every file before accepting it. The exact revisions and
hashes are recorded in `src-tauri/src/translate/catalog.rs` and the validation
record is in `docs/validation/translation-pre-release.md`.

## Distribution boundary

The download travels from Hugging Face to the reader's own machine after their
explicit confirmation. LibreTexts Reader distributes the downloader and the
CTranslate2 runtime, not the model weights. If the models are ever bundled in
the `.app` or mirrored by the project, revisit this file and include all notice
and source obligations that apply to that distribution.
