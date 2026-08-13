# Kokoro runs in the webview; Supertonic runs in Rust

> **Superseded by [ADR-0003](0003-supertonic-is-the-only-bundled-engine.md) (2026-08-13).**
> Kokoro was removed. The reasoning below is kept because it records why the two
> engines sat on opposite sides of the app, and because its open eSpeak NG
> consequence is what ADR-0003 closes.

The two Speech Engines sit on opposite sides of the app for a reason that is not visible in the code. Both could in principle run under Rust's ONNX Runtime, and unifying them would delete a ~2 MiB JavaScript chunk from the bundle — so the asymmetry reads like an accident. It isn't.

Kokoro's ONNX graph takes phoneme ids, not text, so the grapheme-to-phoneme step sits outside the model, and that is the part that does not port. Every existing Rust Kokoro implementation reaches for eSpeak NG, which is GPL-3.0-or-later with no linking exception; linking it into this binary would conflict with distributing the app under Apache-2.0. The permissive alternative — `misaki-rs` with default features disabled — drops out-of-dictionary words to character-by-character spelling, which is a poor trade for an app whose whole purpose is reading textbooks full of proper nouns and technical vocabulary.

Revisit only if bundle size becomes a real constraint, or if a maintained, permissively licensed English G2P library appears in Rust.

## Consequences

- The `SpeechEngine` interface lives in TypeScript, and the Supertonic adapter reaches Rust through a Tauri command.
- Kokoro synthesis cannot be covered by Rust tests. Its adapter is covered through the interface, with a fake.
- The eSpeak NG licensing question is not fully escaped by staying in the webview — `kokoro-js` bundles it as WASM via `phonemizer`, and that ships in the app today. Tracked separately.
