# Supertonic is the only bundled engine

Kokoro never produced audio in a bundled build. Two distinct faults were found, and the
second was never solved.

**Fault 1, root-caused and fixed.** `onnxruntime-web` loads its wasm backend with a dynamic
module `import()`, and `@huggingface/transformers` defaults that path to jsDelivr. The Tauri
CSP allows jsDelivr in `connect-src` but not `script-src`, so the import was blocked. It
worked under `tauri:dev`, where Vite serves `node_modules` as `'self'`, and failed only in a
bundled build — which is why it survived to release.

**Fault 2, never solved.** With the backend loading and the 92 MB model read from disk,
`engine.generate()` hung indefinitely at 0% CPU with zero network sockets, parked on a
promise that never settled. Three hypotheses were tested and all falsified: voice embeddings
fetched at generate time (no sockets ever opened), a missing `espeakng.worker.data` (that
data is inlined in `phonemizer.js` as base64 gzip), and multithreaded wasm starved of
`SharedArrayBuffer` (`numThreads = 1` changed nothing). Do not re-test those three.

Supertonic already ran through the Rust ONNX Runtime, produced audio, and covered playback
and chapter MP3 export. Keeping a second engine that had never worked meant carrying its
model-download subsystem, its 55-voice gallery, its `voices` table and its settings for no
delivered capability.

## Consequences

- `SpeechEngineId` has one member. `createSpeechEngine` keeps its `switch` — see
  ADR-0001's reasoning about where engine choice lives — so adding a provider is still a
  one-case change.
- The voice gallery is gone. Supertonic's ten voice styles are a static list chosen in
  Settings, and nothing about them needs downloading.
- **ADR-0001's open eSpeak NG consequence is closed.** `kokoro-js` bundled eSpeak NG as
  WASM via `phonemizer`, and that shipped in the app. Removing the dependency takes a
  GPL-3.0-or-later payload out of an Apache-2.0 binary. This is the largest effect of the
  change and it is invisible in the diff.
- Playback is single-engine until Fish Audio lands as a bring-your-own-API-key provider.
