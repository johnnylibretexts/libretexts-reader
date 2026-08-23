# Supertonic exports AAC, not MP3

LAME is LGPL. `mp3lame-encoder` vendors it and links it **statically**, and
`Cargo.toml` sets `lto = true` and `strip = true`. Together those make the LGPL
§6 relink right impossible to exercise: there is no shared library to swap, no
object files to relink against, and the shipped binary cannot be relinked from
itself. Complying meant one of dynamic linking, shipping object files, or a
written offer — and the latter two require maintaining a parallel unstripped
build whose only purpose is to be a compliance artifact, indefinitely, for a
right nobody is likely to exercise.

macOS AudioToolbox encodes AAC, is part of the operating system, and carries no
distribution obligation at all. Removing the *reason* the obligation applies is
cheaper than complying with it, and it deletes a bundled dependency rather than
adding one.

**macOS has no MP3 encoder to offer.** AudioToolbox decodes MP3 and will not
encode it. So this is not a swap of encoders behind an unchanged output — the
container necessarily changes, and Supertonic chapter exports are now AAC in an
M4A container.

## Consequences

- **The two providers no longer share a container.** Fish returns MP3 from its
  API and is left alone; re-encoding lossy audio to make the extensions match
  would cost quality and CPU for nothing a reader can hear. `export_extension`
  in `src-tauri/src/tts/provider.rs` is the single declaration of which is
  which, mirrored by `SPEECH_ENGINE_EXPORT_FORMAT` in
  `src/lib/speech/types.ts`. The `TtsProvider::synthesize` doc used to promise
  both implementations returned the same thing so the export path never
  branched — that promise is withdrawn, and anything naming or tagging an
  export must ask.

- **Tagging dispatches on the extension.** ID3 frames do not exist in an MP4
  container, and writing an ID3 tag onto an M4A corrupts it rather than
  failing. `tag_chapter_export` matches explicitly and errors on a container it
  does not know, because silently skipping would drop the licence and
  attribution that #97 added — the exact failure that feature exists to
  prevent. `mp4ameta` (MIT) writes the MP4 side.

- **The TTS cache version moved to `tts-cache-v3`.** The cache key does not
  hash the container, so a v2 entry and its v3 replacement would differ only by
  extension: the old file would never be read again and never be collected
  either. The bump puts them somewhere cleanup can see.

- **The encoder is macOS-only.** CI typechecks the crate on Linux, where
  `encode_f32_to_m4a` is a stub returning an error. The stub exists so call
  sites still resolve and the Linux gate stays honest; the tests that exercise
  real encoding are `#[cfg(target_os = "macos")]`.

- **Exports already on disk are unaffected.** Nothing rewrites or deletes a
  previously exported MP3; it simply will not be found in the cache again.
