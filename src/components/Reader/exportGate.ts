import type { SpeechEngineId } from "../../lib/speech";
import type { SupertonicChapterEstimate } from "../../lib/tauri";

/**
 * Whether a chapter export must stop and ask before it is allowed to run.
 *
 * Pulled out of `SupertonicChapterExport` as a pure function for the same
 * reason `billable_characters` and `provider_for` are pure in Rust: this is
 * the highest-risk decision on the Fish Audio path -- getting it wrong spends
 * the reader's money without asking -- and there is no component-testing
 * library here to drive the component itself. A pure function beside its
 * component can be tested exhaustively; a `useState` read inside JSX cannot.
 *
 * The invariant it exists to hold: **no billed request may proceed on an
 * estimate that does not correspond to the exact chapter, voice and
 * force-flag being requested right now.** That is why a missing estimate
 * gates rather than proceeds -- an absent price is an unknown price, never a
 * free one -- and why a forced Fish export always gates: `force` makes the
 * export skip the cache and re-synthesise, so it bills in full even for a
 * chapter that is already on disk (see `billable_characters` in
 * `src-tauri/src/commands/chapter_tts.rs`).
 */
export function requiresExportConfirmation({
  estimate,
  force,
  provider,
}: {
  /** The estimate fetched for exactly this chapter, voice and `force` flag. */
  estimate: Pick<SupertonicChapterEstimate, "billableCharacters"> | null;
  force: boolean;
  provider: SpeechEngineId;
}): boolean {
  // A local engine costs nothing, in every combination of the other two
  // arguments. Nothing below this line can gate a Supertonic export.
  if (provider !== "fish") {
    return false;
  }

  // Forced regeneration re-synthesises regardless of the cache, so it bills
  // whatever the estimate says. Gating on `force` alone -- rather than
  // trusting a `billableCharacters` that may have been computed with
  // `force: false` -- is what closes the "Regenerate an exported chapter"
  // bypass without depending on the caller having refetched correctly.
  if (force) {
    return true;
  }

  // No estimate yet (first click, or one invalidated by a chapter/voice
  // change and not yet refetched). Treat unknown as billed.
  if (!estimate) {
    return true;
  }

  return estimate.billableCharacters > 0;
}
