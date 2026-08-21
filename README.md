# LibreTexts Reader

LibreTexts Reader is a free, open-source desktop app for listening to OpenStax,
LibreTexts, and Pressbooks textbooks, EPUBs, PDFs, pasted text, and article URLs
with neural TTS. It runs local by default — the bundled **Supertonic** engine
speaks entirely on your machine, with no account, no key, and no network, once
its voice model has been downloaded. You can optionally configure **Fish Audio**,
a cloud voice provider, if you supply your own API key; nothing is sent to Fish
unless you turn it on. See the "Fish Audio" section below before enabling it.

There is **no telemetry and no analytics** of any kind. But the app is not a
zero-network application: importing a book fetches it, and its images, from the
publisher's servers, and Supertonic needs a one-time model download first.
**[PRIVACY.md](PRIVACY.md) lists every host the app can contact and why** — read
that rather than trusting this paragraph.

> **Not affiliated with LibreTexts.** LibreTexts Reader is an independent open-source
> project. It is not affiliated with, endorsed by, or sponsored by LibreTexts or OpenStax.

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

- `target/release/bundle/macos/LibreTexts Reader.app`
- `target/release/bundle/dmg/LibreTexts Reader_<version>_aarch64.dmg`

Supertonic playback and chapter MP3 export run through the Rust ONNX Runtime
backend with on-demand model downloads.

## Fish Audio (optional cloud voice)

Fish Audio is an optional, bring-your-own-key cloud TTS provider. It is off by
default; Supertonic remains the default engine and needs no key and no account.
Supertonic also needs no network once its voice model is present — that model is
a one-time ~383 MB download from `huggingface.co`, started by you from Settings
or by the first press of Play. Enabling Fish is entirely opt-in from Settings.

If you enable Fish, be aware:

- Every synthesis request is sent to Fish Audio's servers over the network,
  along with the text being read. **Fish Audio may retain request data to
  improve their models' quality** — this is a policy of the third-party
  provider, not something this app controls. If you are reading licensed or
  sensitive material aloud, consider that before turning Fish on. See Fish
  Audio's own privacy documentation for their current retention policy.
- Fish Audio synthesis is a paid, metered service. LibreTexts Reader gates
  chapter export behind an explicit cost confirmation, but ordinary playback
  also bills your account as you listen. Playback is not billed
  sentence-by-sentence as you hear it: to keep audio gapless, the player reads
  ahead and synthesizes up to ten sentences at a time, so pressing Play bills
  for roughly ten sentences at once, and so does every seek past what is
  already buffered. Sentences fetched ahead of a passage you skip or a session
  you end are billed whether or not you hear them.
- Your Fish Audio API key is stored in the operating system keychain, never in
  the app's SQLite database or in plain text, and the app has no way to
  display it back to you once saved.

## License

The LibreTexts Reader source code is licensed under Apache-2.0 (see `LICENSE`).

Distributed app bundles also include third-party components under their own
licenses — for example FFmpeg (LGPL) and the PDFium binaries. Their notices are
collected in the `LICENSES/` directory.
