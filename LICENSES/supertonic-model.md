# The Supertonic voice model

The bundled on-device voice is **not** bundled. `Supertone/supertonic-3` is
downloaded from Hugging Face onto the reader's own machine the first time they
press Play, by `src-tauri/src/tts/supertonic/model.rs`. Nothing in the `.app`
contains its weights.

That distinction decides which obligations apply, so it is stated first.

## Terms

| | |
| --- | --- |
| Model | [`Supertone/supertonic-3`](https://huggingface.co/Supertone/supertonic-3) |
| Licence | **OpenRAIL-M** |
| Copyright | © 2026 Supertone Inc. |
| Verified | 2026-08-23, against the repo's own `LICENSE` file |

The model card also notes that Supertone's *sample code* is MIT and that its
PyTorch dependency is BSD-3-Clause. This app uses neither: it loads the ONNX
weights through the `ort` crate and has no Supertone code in it.

## What this means for exported audio

The licence is explicit, and it is the answer to the question a reader is most
likely to have about a chapter they exported:

> Licensor claims no rights in the Output You generate using the Model. You are
> accountable for the Output you generate and its subsequent uses.

So an exported `.m4a` is the reader's, to keep or share. Supertone asserts no
ownership of it and no attribution obligation attaches to it. The
responsibility that does attach is the reader's own: OpenRAIL-M is a
Responsible AI licence, and its **Attachment A — Use Restrictions** governs
what the model may be used *for* — impersonation and disinformation among the
prohibited uses.

## What this means for the app

OpenRAIL-M's distribution obligations are triggered by distributing the model
or a derivative of it:

> You must give any Third Party recipients of the Model or Derivatives of the
> Model a copy of this License

and

> Use-based restrictions as referenced in paragraph 5 MUST be included as an
> enforceable provision by You in any type of legal agreement.

This app distributes neither. It arranges a download, from Supertone's own
repository, over a connection between the reader and Hugging Face — the same
relationship the reader would have downloading it themselves. The licence text
they receive is the one in that repository.

Were the weights ever bundled into the `.app`, that would change: the app would
then be distributing the Model, and both obligations above would attach to the
distribution. **Do not bundle the model without revisiting this file.**

## Where this is recorded elsewhere

- `PRIVACY.md` — that `huggingface.co` is contacted, and when.
- `CLAUDE.md` — that the model is a one-time ~383MB download rather than a
  bundled asset.
- `NOTICE-third-party.md` — the components that genuinely ship, which this is
  deliberately not part of.
