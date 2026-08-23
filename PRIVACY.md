# Privacy

LibreTexts Reader has no account system, no server of its own, and no telemetry. But it
is **not** a zero-network application, and an earlier version of the README wrongly said
it was. This file is the honest version.

## What the app never does

- **No telemetry, analytics, crash reporting, or usage tracking.** There is none in the
  codebase — no Sentry, no analytics SDK, nothing that reports back. Nothing about what
  you read, when you read it, or that you ran the app at all is sent anywhere.
- **No account, no sign-in, no identifier.** The app never asks who you are.
- **Your library never leaves your machine.** Imported books, reading positions, settings,
  and generated audio live only in the app's data directory
  (`~/Library/Application Support/dev.johnnylibretexts.reader` on macOS).
- **Your Fish Audio API key is never displayed, logged, or written to the database.** It
  is stored in the operating system keychain, and there is deliberately no code path that
  returns it to the app's interface — not even to show it back to you.

## Every host the app can contact

All network traffic comes from the app's Rust backend. The interface itself makes no
network requests at all.

| Host | When | Why |
| --- | --- | --- |
| `huggingface.co` | Once, when you download the Supertonic voice model | The on-device speech model (~383 MB). Started by you from Settings, or by the first press of Play. |
| `openstax.org` | Browsing or importing from OpenStax | Catalog listings, book content, images |
| `commons.libretexts.org` and the per-subject `*.libretexts.org` sites | Browsing or importing from LibreTexts | Catalog listings, book content |
| 15 bundled Pressbooks hosts (`milnepublishing.geneseo.edu`, `*.pressbooks.pub`, and other university sites) | Browsing or importing from Pressbooks | Catalog listings, book content. The list is bundled in `src-tauri/resources/catalog/pressbooks-networks.json`; the app refuses any host not on it. |
| **Any host referenced by a book you import** | While importing | Books embed images from wherever their publisher hosts them — CDNs, university servers, equation renderers. See the warning below. |
| **Any URL you paste** | Article import | The app fetches the page you asked for |
| `api.fish.audio` | Only if you save a Fish Audio API key and enable it | Cloud speech synthesis. See below. |

### The one that deserves a warning

**Importing a book contacts whatever hosts that book's pages reference.** Image downloads
are not restricted to an allow-list — the app fetches every image URL the source page
names (`src-tauri/src/content/images.rs`). In practice that means CDNs and equation
renderers such as `files.mtstatic.com` and `quicklatex.com`, but in principle it is
whatever the publisher put in their HTML, including a tracking pixel.

The requests carry no cookies, no account, and nothing identifying you beyond what any
HTTP request unavoidably reveals: your IP address, and the fact that something fetched
that image.

## Supertonic (the default voice) and "offline"

Supertonic runs entirely on your machine and needs no key, no account, and no network —
**after** its model has been downloaded. That download is a one-time ~383 MB fetch from
`huggingface.co`, and until it happens the app cannot speak.

So: offline after setup, not offline on first run. The download is triggered by you, from
Settings or by the first press of Play. Nothing downloads silently in the background.

## Fish Audio (optional, off by default)

Fish Audio is a paid third-party cloud service. It is **off unless you supply your own
API key**, and Supertonic never depends on it.

If you enable it:

- **The text being read is sent to Fish Audio's servers.** That is how the service works.
- **Fish Audio may retain request data to improve their models.** That is their policy,
  not something this app controls. If you are reading licensed, confidential, or personal
  material aloud, weigh that before enabling it. See Fish Audio's own privacy
  documentation for their current terms.
- **It bills your account as you listen.** Not sentence-by-sentence: to keep audio
  gapless the player reads ahead, so each press of Play — and each seek past what is
  already buffered — buys up to three sentences at once, including any you skip past or
  never hear. Pause stops further requests; one already sent cannot be recalled and is
  still charged. Choosing Fish Audio for playback requires confirming all of this first,
  and the same facts sit beside the API key field in Settings.

Your key is stored in the OS keychain, validated once against Fish's wallet endpoint when
you save it, and never written to the app's database.

## Where your data lives

Everything is under the app data directory, and deleting it removes everything the app
knows:

```
~/Library/Application Support/dev.johnnylibretexts.reader
```

The Fish Audio key is the exception — it lives in the OS keychain and is removed from
Settings, or with Keychain Access.

## Questions

This file describes the code as of the date of the release it ships with. If something
here does not match what you observe, that is a bug worth reporting — the network
behaviour above is verifiable in the source, and this document is meant to stay checkable
against it.
