# Johnny Reader

Johnny Reader is a free, open-source desktop app for listening to OpenStax
textbooks, EPUBs, PDFs, pasted text, and article URLs with on-device neural TTS.

## Development

```bash
npm install
npm run tauri:dev
```

## Verification

```bash
npm run build
cargo check -p libretexts-reader
```

## Distribution

```bash
npm run tauri:build
```

On macOS, successful local builds produce:

- `target/release/bundle/macos/Johnny Reader.app`
- `target/release/bundle/dmg/Johnny Reader_0.1.0_aarch64.dmg`

Kokoro playback runs through the bundled webview `kokoro-js` engine and uses
the app-downloaded local model file. Supertonic playback and chapter MP3 export
run through the Rust ONNX Runtime backend with on-demand model downloads.

## License

The Johnny Reader source code is licensed under Apache-2.0 (see `LICENSE`).

Distributed app bundles also include third-party components under their own
licenses — for example FFmpeg (LGPL) and the PDFium binaries. Their notices are
collected in the `LICENSES/` directory.
