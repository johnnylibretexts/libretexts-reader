import { describe, expect, it } from "vitest";
import { requiresExportConfirmation } from "./exportGate";

const CACHED = { billableCharacters: 0 };
const UNCACHED = { billableCharacters: 1234 };

describe("requiresExportConfirmation", () => {
  describe("Fish Audio", () => {
    it("proceeds for a cached chapter that is not being forced", () => {
      // The one Fish case that costs nothing: the file is on disk and the
      // export is a copy. Gating here would train the reader to click
      // through the confirmation without reading it.
      expect(
        requiresExportConfirmation({
          estimate: CACHED,
          force: false,
          provider: "fish",
        }),
      ).toBe(false);
    });

    it("gates a forced regeneration of a cached chapter", () => {
      // `force` skips the cache and re-synthesises: a full, billed request
      // even though `billableCharacters` here says 0, which is exactly the
      // shape of the first bypass this gate shipped with.
      expect(
        requiresExportConfirmation({
          estimate: CACHED,
          force: true,
          provider: "fish",
        }),
      ).toBe(true);
    });

    it("gates an uncached chapter", () => {
      expect(
        requiresExportConfirmation({
          estimate: UNCACHED,
          force: false,
          provider: "fish",
        }),
      ).toBe(true);
    });

    it("gates a forced uncached chapter", () => {
      expect(
        requiresExportConfirmation({
          estimate: UNCACHED,
          force: true,
          provider: "fish",
        }),
      ).toBe(true);
    });

    it("gates when there is no estimate at all", () => {
      // The second bypass: the very first click, before any estimate has
      // resolved, read `estimate?.billableCharacters ?? 0` as 0 and sent an
      // unconfirmed billed request. An absent price is an unknown price.
      expect(
        requiresExportConfirmation({
          estimate: null,
          force: false,
          provider: "fish",
        }),
      ).toBe(true);
      expect(
        requiresExportConfirmation({
          estimate: null,
          force: true,
          provider: "fish",
        }),
      ).toBe(true);
    });
  });

  describe("Supertonic", () => {
    it.each([
      ["cached, not forced", CACHED, false],
      ["cached, forced", CACHED, true],
      ["uncached, not forced", UNCACHED, false],
      ["uncached, forced", UNCACHED, true],
      ["no estimate, not forced", null, false],
      ["no estimate, forced", null, true],
    ])("proceeds when %s", (_name, estimate, force) => {
      // Supertonic runs on-device and bills nothing, ever. A confirmation
      // dialog on a free action is noise that devalues the real one.
      expect(
        requiresExportConfirmation({
          estimate,
          force,
          provider: "supertonic",
        }),
      ).toBe(false);
    });
  });
});
