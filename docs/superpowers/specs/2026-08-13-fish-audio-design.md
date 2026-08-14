# Fish Audio as an optional TTS provider — design

**Date:** 2026-08-13
**Status:** approved, not yet implemented

Supertonic is the only speech engine after the Kokoro removal (ADR-0003). Add **Fish Audio**
as a second, optional provider where the user supplies their own API key. Supertonic stays
the default and the only engine that works with no account and no network.

This is the "spec B" referred to in `HANDOFF.md`. Spec A (remove Kokoro) shipped as
`09d97b3`.

## Scope

**In scope**

- Fish Audio for **both** live playback and chapter MP3 export — full parity with Supertonic.
- A Settings surface to paste, validate, replace and clear the API key.
- Voice selection from the account's own Fish voice models, or a pasted public voice id.
- A confirmation gate before any paid export run.
- Honest failure behaviour when the network, the key, or the account fails.

**Out of scope**

- Voice cloning, voice design, ASR, and multi-speaker dialogue. Fish offers all of these;
  none of them serve reading a textbook aloud.
- The WebSocket streaming endpoint (`/v1/tts/live`). It exists for streaming an LLM's tokens
  into speech as they arrive. This app always has the whole sentence before it asks for
  audio, so plain `POST /v1/tts` is the right call and the simpler one.
- Any change to how Supertonic works.

## Decisions

| Question | Decision | Why |
|---|---|---|
| Which surfaces? | Playback **and** chapter export | Parity; export is where voice quality matters most and is most durable |
| Where does the key live? | OS keychain via the `keyring` crate | Never in `library.sqlite`, which `HANDOFF.md:59` tells users to copy between machines |
| Which Fish model? | `s2.1-pro`, the paid tier | The free `s2.1-pro-free` tier expires 2026-08-31 — 18 days after this spec — and designing around a promotion that lapses that soon is not worth the complexity |
| How is paid export gated? | Estimate plus explicit confirm | One click could otherwise bill a whole textbook |
| What happens on failure? | Stop, name the cause, offer to switch | The user must always know which engine they are hearing and why it changed |
| How does Rust dispatch? | A `TtsProvider` trait | Export must dispatch by provider regardless; without a seam that `match` is duplicated in the playback path and the export path, and the copies drift |

### Why the key is not a settings row

Every other setting is a plain row in the SQLite `settings` table, and that is the path of
least resistance here too. It is rejected because of a workflow this project actively
documents: `HANDOFF.md:59` instructs people to copy the whole app-data directory between
machines to preserve a test library, and the same directory is what anyone would hand over
to reproduce a bug. A key stored there travels with it, silently. The threat is not an
attacker with disk access — it is the app's own documented ergonomics.

`keyring` maps to macOS Keychain, Windows Credential Manager and Linux Secret Service. The
Linux path needs a running secret-service daemon; this app is macOS-first, and a missing
daemon must surface as a clear "cannot store the key on this system" error rather than a
silent fall back to plaintext.

## Architecture

Fish synthesis runs **in Rust**, not the webview. Chapter export already lives in Rust, so
parity requires it — and it has a second benefit: the API key never crosses into the
webview at all.

```
                    ┌─────────────── frontend ───────────────┐
  player ──────────►│ createSpeechEngine (src/lib/speech)     │
  chapter export ──►│   supertonicEngine.ts │ fishEngine.ts   │  both only `invoke`
                    └──────────────────┬─────────────────────┘
                                       │ Tauri invoke: text, voice, language
                    ┌──────────────────▼─────────────────────┐
                    │ commands/tts.rs · commands/chapter_tts.rs│
                    │   cache · estimates · progress · gate   │  ← shared, provider-agnostic
                    │                  │                      │
                    │        dyn TtsProvider                  │
                    │        ├── SupertonicProvider (ort)     │
                    │        └── FishProvider (reqwest)       │
                    └─────────────────────────────────────────┘
                                       │ HTTPS (not subject to the webview CSP)
                                       ▼
                              api.fish.audio
```

### Dispatch reads the request, never the settings table

"Rust dispatches by provider" must not be read as "Rust looks up `tts_provider`". The doc
comment on `commands::tts::synthesize_speech` records that the command **deliberately stopped**
reading that setting: the webview already decides which engine speaks, and deciding a second
time in Rust from a different source, with no ordering guarantee between the two, is the bug
that change removed. It also made the command fail by default, because the seeded provider was
not the one it served.

So the provider travels as a **field on the request**. Rust builds the provider the caller
named and never re-derives it. An unknown name is an error, not a fall back to a default — a
silent default would let a frontend bug switch engines invisibly, which is the same class of
failure again.

`commands/supertonic_tts.rs` is renamed to `commands/chapter_tts.rs` as part of this work: it
becomes the shared, provider-agnostic export path and keeping a provider's name on it would
be actively misleading. `SupertonicChapterEstimate` and its siblings lose the prefix for the
same reason.

**No CSP change is required.** `HANDOFF.md` lists adding `https://api.fish.audio` to
`connect-src` as a known task; that applies only to a webview-based client. Rust's HTTP
client is not subject to the webview CSP. Do not add it — an unnecessary `connect-src`
entry widens what the webview may reach for no benefit.

### The trait

```rust
#[async_trait]
pub trait TtsProvider: Send + Sync {
    fn id(&self) -> &'static str;

    /// Encoded MP3 bytes. Both providers return the same thing so the export
    /// path never branches on which one produced the audio.
    async fn synthesize(&self, text: &str, voice: &str, language: &str) -> AppResult<Vec<u8>>;

    /// Usable? Downloads models for Supertonic; checks the key for Fish.
    async fn ensure_ready(&self) -> AppResult<()>;

    async fn list_voices(&self) -> AppResult<Vec<VoiceSummary>>;
}
```

Deliberately narrow. Caching, progress events, estimates and the confirm gate stay **outside**
the trait in shared export code. Supertonic reports chunk-level progress and Fish returns a
whole utterance; a trait that tried to model both notions of progress would leak one into the
other. Supertonic's implementation keeps its existing ffmpeg/mp3lame encode step so that
returning MP3 is uniform.

This mirrors the frontend's `SpeechEngine` (`src/lib/speech/types.ts`), which is already
documented as the single place an engine is chosen. Two layers, one idea.

## Key storage and the command surface

Three commands. **There is no getter.**

| Command | Returns | Notes |
|---|---|---|
| `set_fish_api_key(key)` | `FishKeyStatus` | Validates before storing; stores only on success |
| `clear_fish_api_key()` | `()` | Removes the keychain entry |
| `fish_key_status()` | `FishKeyStatus` | `{ present: bool, valid: Option<bool>, credit: Option<f64> }` |

The frontend can learn that a key is present and whether it validated. It can never read the
key back. A getter would put the secret into the webview and into any devtools session, which
is the one thing this design exists to prevent.

The `settings` table gains `fish_voice_id` only. It never holds the key.

### Validating without spending money

Validate with `GET /wallet/self/api-credit`, not a test synthesis:

- `200` — the key works, and the response carries the credit balance for the export gate.
- `401` — bad key. Reject it and do not store it.

A test synthesis would also prove the key works, and would bill for the privilege. Every
key entry and re-validation would cost the user money.

## Voice selection

`GET /model?self=true&page_size=…` lists the account's own voice models. The Settings panel
shows them as a list; a text field also accepts a public voice id pasted from fish.audio,
since the interesting narration voices are public models the user does not own.

The chosen id is stored as `fish_voice_id` and sent as `reference_id` on every request.

`SpeechEngine.defaultVoice` exists because the player carries one voice id across engines and
may hand an engine an id belonging to another. Fish has no sensible built-in default voice —
if `fish_voice_id` is unset, `ensure_ready` fails with a message telling the user to choose
one, rather than guessing.

## The export gate

`SupertonicChapterEstimate` already carries `word_count`, `estimated_seconds` and `cached`,
and is already shown before an export runs. Extend it, and rename it to drop the
provider-specific name:

- **billable characters** — the sum of the text actually sent, excluding cached chapters
- **cached chapters** — how many cost nothing because the content-addressed cache already
  holds them
- **credit balance** — from `/wallet/self/api-credit`

The confirmation names the provider and the billable character count, and must be accepted
before any paid request is made.

**Do not hardcode per-character pricing.** A price baked into the app goes stale silently and
then lies to the user about what they are about to spend. Characters and the live credit
balance are both facts we can state truthfully and will stay true.

## Cache correctness

The content-addressed cache key (`tts/supertonic/cache.rs::cache_path_in`) currently hashes
the cache version, model version, step count, voice style, language, document id, section id
and text. It **must** gain:

- the provider id
- the Fish model string (`s2.1-pro`)
- the voice `reference_id`

Without this, a chapter exported with Supertonic would be served from cache for a Fish
request — identical text and language, identical key — and the user would get the wrong
voice with no error. The cache version constant must be bumped in the same change so
existing entries do not collide with the new scheme.

The model string must be a **named constant** used by both the request header and the cache
key, never a literal at the call site. Fish selects the model by HTTP header and falls back
to `s2.1-pro` when that header is missing or unrecognised. Choosing `s2.1-pro` means a typo
happens to land on the intended model today — but that is luck, not safety. The moment the
model changes, a mistyped literal would silently serve a different model while the cache,
keyed on the intended value, hid the discrepancy behind cache hits forever. One constant,
two uses.

## Error handling

Fish's HTTP errors map onto `AppError`:

| HTTP | Meaning | Retryable |
|---|---|---|
| `401` | Missing or bad key | No — prompt to re-enter |
| `402` | Out of credit | No — prompt to top up |
| `422` | Invalid request | No — a bug in our request shaping |
| `429` | Rate limited | Yes, but not silently — see below |
| network / timeout | Transient | Yes |

Any new `AppError` variant must be mirrored in `src/types/domain.ts` or
`scripts/ci/check-error-kinds.sh` fails CI. That gate is the reason this is a checklist item
rather than a footnote.

**On failure during playback:** stop, surface a message naming the cause, and offer a
one-click switch to Supertonic. Do not silently fall back. A silent fallback means the user
cannot tell a paid engine from a free one by ear, and a permanently broken key would go
unnoticed indefinitely.

Fish publishes no SLA on latency. Requests need the same 15–20s timeouts used elsewhere in
the app, and a timeout is a stop-and-surface, not a hang.

## Documentation that must change

- **`CLAUDE.md`** and the **README** both claim the app is on-device / offline by design.
  That stops being unconditionally true. Reword to: local by default, with one optional
  cloud provider the user must configure. The app must still work fully offline on
  Supertonic.
- **`HANDOFF.md`**'s note that `api.fish.audio` needs adding to `connect-src` is obsolete
  under this design and should be removed so nobody widens the CSP for no reason.
- A note that Fish requests may be retained by the provider to improve model quality — a
  user reading licensed material aloud should be able to find that out from our docs, not
  only from Fish's blog.

## Testing

**Rust, no network:**

- `TtsProvider` dispatch picks the provider named in settings, for playback and export alike.
- Cache keys differ across providers, across voice ids, and across model strings for
  otherwise identical input. This is the regression test for the wrong-voice-from-cache bug.
- HTTP status → `AppError` kind mapping, table-driven over the statuses above.
- Request shaping: the `model` header equals the named constant; `reference_id` is the
  stored voice id.

Use a stub `TtsProvider` and a local HTTP fixture. **No test that runs by default may call
`api.fish.audio`** — it would need a real key and would bill whoever ran the suite. A live
smoke test may exist only as `#[ignore]`d, matching
`live_imports_small_public_book_with_images`, and its doc comment must say that running it
costs money.

Tests must not touch `paths::` helpers — see the app-data isolation gate.

**Frontend:**

- `createSpeechEngine` returns the Fish engine when the provider setting says so.
- Key status drives the Settings UI: absent, present-and-valid, present-and-invalid.
- The confirm gate blocks export until accepted.

**Manual, needs a real key and will cost a little money:**

- Paste a bad key → rejected at entry, nothing stored.
- Paste a good key → validates, credit balance shown.
- Play a section on Fish, confirm the chosen voice is what is heard.
- Export a two-chapter section, confirm the gate names the right character count, then
  confirm re-export is free and reported as cached.
- Pull the network mid-playback → playback stops, names the cause, offers Supertonic.

## Follow-on work, deliberately not here

- **Paid-tier consent beyond the export gate.** Live playback bills per sentence with no
  confirmation. That is defensible because the user chose the provider and can hear it
  working, but a running-cost indicator would be kinder.
- **Cancellation.** `SpeechEngine` documents that synthesis in flight cannot be aborted
  because the Rust command has no cancellation channel. With a paid provider, an
  un-cancellable export is money that cannot be stopped once started. This belongs with the
  cancellation work already carried in the imports notes.
- **The free tier.** `s2.1-pro-free` exists and is free until 2026-08-31 under a fair-use
  policy with no SLA. Deliberately not used: it lapses too soon to design around. If Fish
  makes it permanent, revisit — but note the trap, that an unrecognised `model` header falls
  back to the **paid** model, so a free-tier implementation must verify which model actually
  served the request rather than trusting the one it asked for.
