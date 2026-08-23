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

> **Developed in partnership with LibreTexts.** LibreTexts Reader is an open-source
> project developed by johnnylibretexts in partnership with LibreTexts, who hold the
> copyright. It is not affiliated with, endorsed by, or sponsored by OpenStax or
> Pressbooks, which are separate organisations, nor by the individual Pressbooks
> networks it imports from.

## Known limitations

Stated here rather than left to be discovered, because each one is invisible in
the app and reads as a bug rather than a boundary.

- **Books are imported as a reading flow, not a copy of the page.** A table is
  replaced by a note saying one was omitted; sidebars and exercises may not be
  carried across at all. In a STEM chapter that can be a real fraction of the
  page.
- **Equations are read aloud approximately**, and some are read only as
  "equation". They are typeset correctly on screen with KaTeX — it is the
  *speech* that is heuristic, not the display. This is not accessibility-grade
  math narration.
- **There is no search inside a book and no bookmarks.** Library search matches
  book titles only. A book does reopen where you left it. See
  [ADR 0005](docs/adr/0005-in-book-search-is-deferred.md) for why this is
  deferred rather than shipped small.
- **There are no in-app updates.** The auto-updater is deliberately absent, so a
  new version means downloading a new build.

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
- Fish Audio synthesis is a paid, metered service. Selecting it for playback
  requires an explicit confirmation that states this, as does chapter export
  and the Settings voice test — but once selected, ordinary playback bills your
  account as you listen. Playback is not billed sentence-by-sentence as you
  hear it: to keep audio gapless, the player reads ahead, so pressing Play buys
  up to three sentences at once, and so does every seek past what is already
  buffered. Sentences fetched ahead of a passage you skip or a session you end
  are billed whether or not you hear them. Pause stops further requests, but a
  request already sent cannot be recalled and is still charged.
- Your Fish Audio API key is stored in the operating system keychain, never in
  the app's SQLite database or in plain text, and the app has no way to
  display it back to you once saved.

## License

The LibreTexts Reader source code is licensed under Apache-2.0 (see `LICENSE`).

Distributed app bundles also include third-party components under their own
licenses. `LICENSES/NOTICE-third-party.md` attributes every one of them —
generated from the lockfiles by `scripts/generate-notices.sh`, and shipped
inside the `.app`. The bundled native components (the PDFium binaries, and the
`id3` and `mp4ameta` crates that write attribution into exported audio) carry
their full notices as separate files in the same directory.

The on-device voice model is downloaded to your machine rather than distributed
with the app; its terms, and what they mean for audio you export, are in
`LICENSES/supertonic-model.md`.
