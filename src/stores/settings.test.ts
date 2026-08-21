import { afterEach, describe, expect, it, vi } from "vitest";

/**
 * `setTheme`, `setTtsProvider`, and `saveTtsSettings` all reject on a failed
 * persist. A caller awaiting one of these detects its own failure via an
 * ordinary try/catch -- reading the shared `error` field instead is
 * unreliable, since a concurrent unrelated action can overwrite or clear it
 * before the read happens (see FishAudioSettings.tsx's persistVoice, a real
 * caller built around this).
 *
 * The first two also record the message into that shared field, because the
 * controls that call them render no error of their own. `saveTtsSettings`
 * deliberately does not: both of its callers render what they catch beside
 * their own control, so a copy in the store would be a message nothing shows.
 *
 * `setTheme` sets its field optimistically before the persist call, so a
 * failed save must revert it -- otherwise the store (and the UI reading it)
 * keeps claiming a value that was never written to disk, disagreeing with
 * what the next app start loads. Its revert test seeds a value deliberately
 * different from the one being set, so a missing (or no-op) revert cannot
 * pass by coincidence.
 *
 * `setTtsProvider` and `saveTtsSettings` do not set optimistically at all:
 * they apply only once the write has landed, so there is no window to revert
 * and no way for the store to disagree with the DB in either direction.
 * Playback builds its engine from those rows at every sentence boundary,
 * which is why they are stricter than the theme.
 *
 * Every module under test is re-imported fresh per test (`vi.resetModules`)
 * so each gets its own mocked `api.setSetting`.
 */
async function loadSettingsStore(setSetting: (key: string, value: unknown) => Promise<void>) {
  vi.resetModules();
  vi.doMock("../lib/tauri", () => ({
    api: {
      setSetting,
      getAllSettings: vi.fn(async () => ({})),
    },
  }));

  const { useSettingsStore } = await import("./settings");
  return useSettingsStore;
}

afterEach(() => {
  vi.doUnmock("../lib/tauri");
});

describe("hydrate", () => {
  it("can be retried after a failed load", async () => {
    // A failed load leaves every row at DEFAULT_SETTINGS with `hydrated`
    // true, and playback now builds its engine from two of those rows -- so
    // one transient `get_all_settings` failure otherwise means the whole
    // session reads in "M1", and a Fish reader is silently moved onto
    // Supertonic and asked to download its model. Without a retry the only
    // way out is restarting the app.
    const getAllSettings = vi
      .fn()
      .mockRejectedValueOnce(new Error("database is locked"))
      .mockResolvedValueOnce({ supertonic_voice_style: "F3" });
    vi.resetModules();
    vi.doMock("../lib/tauri", () => ({
      api: { setSetting: vi.fn(async () => undefined), getAllSettings },
    }));
    const { useSettingsStore } = await import("./settings");

    await useSettingsStore.getState().hydrate();
    expect(useSettingsStore.getState().hydrateFailed).toBe(true);
    expect(useSettingsStore.getState().supertonicVoiceStyle).toBe("M1");

    await useSettingsStore.getState().hydrate();

    expect(useSettingsStore.getState().hydrateFailed).toBe(false);
    expect(useSettingsStore.getState().supertonicVoiceStyle).toBe("F3");
  });

  it("keeps rows that were written while a retry was in flight", async () => {
    // Retrying makes `hydrate` race the writes the rest of this store orders
    // carefully. Its read is a snapshot from before any write that lands
    // during it, so applying it wholesale reverts that write in the store
    // while SQLite keeps it -- the divergence `setTtsProvider` and
    // `saveTtsSettings` were both reshaped to prevent.
    let land = () => {};
    const landed = new Promise<void>((resolve) => {
      land = resolve;
    });
    const getAllSettings = vi.fn(async () => {
      await landed;
      return { tts_provider: "supertonic" };
    });
    vi.resetModules();
    vi.doMock("../lib/tauri", () => ({
      api: { setSetting: vi.fn(async () => undefined), getAllSettings },
    }));
    const { useSettingsStore } = await import("./settings");

    const loading = useSettingsStore.getState().hydrate();
    await useSettingsStore.getState().setTtsProvider("fish");
    land();
    await loading;

    expect(useSettingsStore.getState().ttsProvider).toBe("fish");
  });

  it("keeps a theme written while a retry was in flight", async () => {
    // `setTheme` is the one writer that still sets optimistically, so its
    // value can be overwritten by a load that resolves in between -- and
    // recording the write without re-applying it leaves the store and the
    // rendered theme on the old value while SQLite and localStorage hold the
    // new one, flipping at the next launch.
    // The load has to resolve *between* the optimistic set and the write
    // landing -- that is the whole window. Slow write, fast read.
    let land = () => {};
    const landed = new Promise<void>((resolve) => {
      land = resolve;
    });
    const setSetting = vi.fn(async () => {
      await landed;
    });
    const getAllSettings = vi.fn(async () => ({ theme: "dark" }));
    vi.resetModules();
    vi.doMock("../lib/tauri", () => ({ api: { setSetting, getAllSettings } }));
    const { useSettingsStore } = await import("./settings");

    const loading = useSettingsStore.getState().hydrate();
    const theming = useSettingsStore.getState().setTheme("light");
    await loading;
    land();
    await theming;

    expect(useSettingsStore.getState().theme).toBe("light");
  });

  it("settles the theme on the last click, not the last write to resolve", async () => {
    // The Sidebar's theme buttons are neither awaited nor disabled, so two
    // clicks inside one round trip are two concurrent writes. Whichever
    // resolves last used to be the one applied -- the UI snapping back to the
    // earlier click while localStorage kept the later one, and a failed load
    // then rendering that stale theme at the next launch.
    const order: string[] = [];
    const setSetting = vi.fn(async (_key: string, value: unknown) => {
      order.push(String(value));
      // The first write is the slow one, so unserialized calls come back out
      // of order.
      if (order.length === 1) {
        await new Promise((resolve) => setTimeout(resolve, 20));
      }
    });
    vi.resetModules();
    vi.doMock("../lib/tauri", () => ({
      api: { setSetting, getAllSettings: vi.fn(async () => ({})) },
    }));
    const { useSettingsStore } = await import("./settings");

    const first = useSettingsStore.getState().setTheme("light");
    const second = useSettingsStore.getState().setTheme("dark");
    await Promise.all([first, second]);

    expect(order).toEqual(["light", "dark"]);
    expect(useSettingsStore.getState().theme).toBe("dark");
    expect(window.localStorage.getItem("johnny-reader-theme")).toBe("dark");
  });

  it("clears the banner when a later theme click succeeds after an earlier one failed", async () => {
    // A superseded call still reports its own failure, which is right -- but
    // the click that replaced it succeeded, so leaving the banner up tells
    // the reader their theme did not save when it did.
    let attempt = 0;
    const setSetting = vi.fn(async () => {
      attempt += 1;
      if (attempt === 1) {
        throw new Error("disk full");
      }
    });
    vi.resetModules();
    vi.doMock("../lib/tauri", () => ({
      api: { setSetting, getAllSettings: vi.fn(async () => ({})) },
    }));
    const { useSettingsStore } = await import("./settings");

    const first = useSettingsStore.getState().setTheme("light").catch(() => {});
    const second = useSettingsStore.getState().setTheme("dark");
    await Promise.all([first, second]);

    expect(useSettingsStore.getState().theme).toBe("dark");
    expect(useSettingsStore.getState().error).toBeNull();
  });

  it("keeps a committed theme that a later click superseded and then failed", async () => {
    // `recordWrite` answers "this value is on disk", which a superseded write
    // that committed still is -- unlike the store apply, which is guarded so
    // an older click cannot repaint over a newer one. Gating both together
    // left the committed theme unrecorded, so the load reverted it and the
    // store, localStorage and SQLite all disagreed.
    let land = () => {};
    const landed = new Promise<void>((resolve) => {
      land = resolve;
    });
    let attempt = 0;
    const setSetting = vi.fn(async () => {
      attempt += 1;
      if (attempt === 2) {
        throw new Error("disk full");
      }
    });
    const getAllSettings = vi.fn(async () => {
      await landed;
      return { theme: "system" };
    });
    vi.resetModules();
    vi.doMock("../lib/tauri", () => ({ api: { setSetting, getAllSettings } }));
    const { useSettingsStore } = await import("./settings");

    const loading = useSettingsStore.getState().hydrate();
    const dark = useSettingsStore.getState().setTheme("dark");
    const light = useSettingsStore.getState().setTheme("light").catch(() => {});
    await Promise.all([dark, light]);
    land();
    await loading;

    // "dark" is what committed and what localStorage holds.
    expect(useSettingsStore.getState().theme).toBe("dark");
  });

  it("reverts a failed theme to what is committed, not to what was on screen", async () => {
    // `previousTheme` is captured before the write, so a load resolving in
    // between makes it stale: reverting to it puts the store and localStorage
    // on a value SQLite never held -- and `hydrate`'s failure path reads that
    // localStorage back at the next launch.
    // The load has to resolve after the click captured its "previous" value
    // and before the write fails -- that is the window.
    let land = () => {};
    const landed = new Promise<void>((resolve) => {
      land = resolve;
    });
    const setSetting = vi.fn(async () => {
      await landed;
      throw new Error("disk full");
    });
    const getAllSettings = vi.fn(async () => ({ theme: "light" }));
    vi.resetModules();
    vi.doMock("../lib/tauri", () => ({ api: { setSetting, getAllSettings } }));
    const { useSettingsStore } = await import("./settings");

    const loading = useSettingsStore.getState().hydrate();
    const theming = useSettingsStore
      .getState()
      .setTheme("dark")
      .catch(() => {});
    await loading;
    expect(useSettingsStore.getState().theme).toBe("light");

    land();
    await theming;

    expect(useSettingsStore.getState().theme).toBe("light");
    expect(window.localStorage.getItem("johnny-reader-theme")).toBe("light");
  });

  it("reverts to a theme committed during a load, not to what the load returned", async () => {
    // `committedTheme` has to agree with how the load is applied: newer
    // writes win over the read. Preferring the read makes it name a value
    // SQLite no longer holds, and the next failed click reverts the store and
    // localStorage onto it -- which `hydrate`'s failure path reads back.
    let land = () => {};
    const landed = new Promise<void>((resolve) => {
      land = resolve;
    });
    let attempt = 0;
    const setSetting = vi.fn(async () => {
      attempt += 1;
      if (attempt === 2) {
        throw new Error("disk full");
      }
    });
    const getAllSettings = vi.fn(async () => {
      await landed;
      return { theme: "light" };
    });
    vi.resetModules();
    vi.doMock("../lib/tauri", () => ({ api: { setSetting, getAllSettings } }));
    const { useSettingsStore } = await import("./settings");

    const loading = useSettingsStore.getState().hydrate();
    await useSettingsStore.getState().setTheme("dark");
    land();
    await loading;

    await useSettingsStore
      .getState()
      .setTheme("system")
      .catch(() => {});

    expect(useSettingsStore.getState().theme).toBe("dark");
    expect(window.localStorage.getItem("johnny-reader-theme")).toBe("dark");
  });

  it("setTheme leaves another action's banner alone, landing or not", async () => {
    // The provider control has no error line of its own, so the shared field
    // is the only place its failure is reported. Neither clicking nor
    // succeeding makes that someone else's to clear -- nothing about the
    // other action has been fixed either way.
    let land = () => {};
    const landed = new Promise<void>((resolve) => {
      land = resolve;
    });
    const setSetting = vi.fn(async () => {
      await landed;
    });
    vi.resetModules();
    vi.doMock("../lib/tauri", () => ({
      api: { setSetting, getAllSettings: vi.fn(async () => ({})) },
    }));
    const { useSettingsStore } = await import("./settings");

    useSettingsStore.setState({ error: "database is locked" });
    const theming = useSettingsStore.getState().setTheme("dark");

    expect(useSettingsStore.getState().error).toBe("database is locked");

    land();
    await theming;

    expect(useSettingsStore.getState().error).toBe("database is locked");
  });

  it("does not let one action clear another's banner", async () => {
    // The theme and the provider use different row queues, so they run
    // concurrently -- and the Sidebar's theme buttons swallow their rejection
    // and rely entirely on this field. A provider switch landing afterwards
    // cleared it, so the reader watched the theme snap back with no message.
    let landProvider = () => {};
    const providerLanded = new Promise<void>((resolve) => {
      landProvider = resolve;
    });
    const setSetting = vi.fn(async (key: string) => {
      if (key === "tts_provider") {
        await providerLanded;
        return;
      }
      throw new Error("disk full");
    });
    vi.resetModules();
    vi.doMock("../lib/tauri", () => ({
      api: { setSetting, getAllSettings: vi.fn(async () => ({})) },
    }));
    const { useSettingsStore } = await import("./settings");

    const switching = useSettingsStore.getState().setTtsProvider("fish");
    await useSettingsStore
      .getState()
      .setTheme("dark")
      .catch(() => {});
    expect(useSettingsStore.getState().error).toBe("disk full");

    landProvider();
    await switching;

    expect(useSettingsStore.getState().ttsProvider).toBe("fish");
    expect(useSettingsStore.getState().error).toBe("disk full");
  });

  it("never shows a provider the reader has already clicked past", async () => {
    // Only the button being written is disabled, so clicking one provider
    // then the other is an invited interaction -- and the MiniPlayer keeps
    // playing while the reader is in Settings. Applying the first click when
    // its write lands puts "fish" in the store for the length of the second
    // write, and `activeEngine` reads the store live: a sentence boundary in
    // that window builds the Fish engine and issues a billed request for a
    // provider already moved off, filed under a key nothing will read again.
    let landSecond = () => {};
    const secondLanded = new Promise<void>((resolve) => {
      landSecond = resolve;
    });
    const setSetting = vi.fn(async (_key: string, value: unknown) => {
      if (value === "supertonic") {
        await secondLanded;
      }
    });
    vi.resetModules();
    vi.doMock("../lib/tauri", () => ({
      api: { setSetting, getAllSettings: vi.fn(async () => ({})) },
    }));
    const { useSettingsStore } = await import("./settings");

    const first = useSettingsStore.getState().setTtsProvider("fish");
    const second = useSettingsStore.getState().setTtsProvider("supertonic");
    await first;

    expect(useSettingsStore.getState().ttsProvider).not.toBe("fish");

    landSecond();
    await second;

    expect(useSettingsStore.getState().ttsProvider).toBe("supertonic");
  });

  it("shows the provider that committed when the click replacing it fails", async () => {
    // The supersession guard skips the apply and `recordWrite` runs anyway,
    // so a superseded write that *commits* followed by one that *fails*
    // applies nothing at all: SQLite holds Fish, the store says Supertonic,
    // and the banner says the switch failed. The next launch then starts on a
    // provider the screen said was not selected -- and bills through it.
    const setSetting = vi.fn(async (_key: string, value: unknown) => {
      if (value === "supertonic") {
        throw new Error("database is locked");
      }
    });
    vi.resetModules();
    vi.doMock("../lib/tauri", () => ({
      api: { setSetting, getAllSettings: vi.fn(async () => ({})) },
    }));
    const { useSettingsStore } = await import("./settings");

    const first = useSettingsStore.getState().setTtsProvider("fish");
    const second = useSettingsStore
      .getState()
      .setTtsProvider("supertonic")
      .catch(() => {});
    await Promise.all([first, second]);

    // "fish" is what is on disk, so it is what the store has to say.
    expect(useSettingsStore.getState().ttsProvider).toBe("fish");
    expect(useSettingsStore.getState().error).toBe("database is locked");
  });

  it("keeps rows already written when a retry fails", async () => {
    // The failure path reset every row to DEFAULT_SETTINGS, which was
    // harmless while hydrate ran once. Retried, it reverts writes that did
    // commit: the store says Supertonic, SQLite says Fish, and the next
    // launch flips back.
    const getAllSettings = vi
      .fn()
      .mockRejectedValue(new Error("database is locked"));
    vi.resetModules();
    vi.doMock("../lib/tauri", () => ({
      api: { setSetting: vi.fn(async () => undefined), getAllSettings },
    }));
    const { useSettingsStore } = await import("./settings");

    await useSettingsStore.getState().hydrate();
    await useSettingsStore.getState().setTtsProvider("fish");
    await useSettingsStore.getState().hydrate();

    expect(useSettingsStore.getState().ttsProvider).toBe("fish");
  });
});

describe("settings store persistence failures", () => {
  it("saveTtsSettings rejects when the persist fails", async () => {
    const setSetting = vi.fn(async () => {
      throw new Error("disk full");
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    await expect(
      useSettingsStore.getState().saveTtsSettings({ supertonicLanguage: "en" }),
    ).rejects.toThrow("disk full");
  });

  it("saveTtsSettings resolves on success and leaves an unrelated banner alone", async () => {
    const setSetting = vi.fn(async () => undefined);
    const useSettingsStore = await loadSettingsStore(setSetting);

    // A banner from some other action -- a failed theme save, say. That
    // failure is still true after this one succeeds, so clearing it would be
    // this call speaking for an action it knows nothing about.
    useSettingsStore.setState({ error: "stale error from another action" });

    await expect(
      useSettingsStore.getState().saveTtsSettings({ supertonicLanguage: "en" }),
    ).resolves.toBeUndefined();

    expect(useSettingsStore.getState().error).toBe(
      "stale error from another action",
    );
  });

  it("setTheme rejects and records the error when the persist fails", async () => {
    const setSetting = vi.fn(async () => {
      throw new Error("disk full");
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    useSettingsStore.setState({ error: "stale error from another action" });

    await expect(
      useSettingsStore.getState().setTheme("dark"),
    ).rejects.toThrow("disk full");

    expect(useSettingsStore.getState().error).toBe("disk full");
  });

  it("setTtsProvider rejects and records the error when the persist fails", async () => {
    const setSetting = vi.fn(async () => {
      throw new Error("disk full");
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    useSettingsStore.setState({ error: "stale error from another action" });

    await expect(
      useSettingsStore.getState().setTtsProvider("fish"),
    ).rejects.toThrow("disk full");

    expect(useSettingsStore.getState().error).toBe("disk full");
  });

  it("saveTtsSettings leaves the Supertonic voice style alone when the persist fails", async () => {
    // A failed save must not change the voice actually spoken. That was
    // harmless while playback ignored the style; now it would speak in a
    // voice the DB never stored for the rest of the session, until the next
    // launch silently changed it back. Seeded to "M1" -- different from the
    // "F3" being saved -- so a no-op could not pass by coincidence.
    const setSetting = vi.fn(async () => {
      throw new Error("disk full");
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    useSettingsStore.setState({ supertonicVoiceStyle: "M1" });

    await expect(
      useSettingsStore
        .getState()
        .saveTtsSettings({ supertonicVoiceStyle: "F3" }),
    ).rejects.toThrow("disk full");

    expect(useSettingsStore.getState().supertonicVoiceStyle).toBe("M1");
  });

  it("saveTtsSettings applies nothing when a write throws instead of rejecting", async () => {
    // `Promise.allSettled` only settles promises it is handed. A `setSetting`
    // that throws synchronously throws inside the `.map` that builds that
    // array, before `allSettled` is ever called -- so it escaped the whole
    // settled-results path, taking the rejection with it. Nothing must be
    // applied on that path either.
    const setSetting = vi.fn((_key: string, _value: unknown): Promise<void> => {
      throw new Error("invoke unavailable");
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    useSettingsStore.setState({ supertonicVoiceStyle: "M1" });

    await expect(
      useSettingsStore
        .getState()
        .saveTtsSettings({ supertonicVoiceStyle: "F3" }),
    ).rejects.toThrow("invoke unavailable");

    expect(useSettingsStore.getState().supertonicVoiceStyle).toBe("M1");
  });

  it("saveTtsSettings leaves the shared banner to actions with nowhere else to report", async () => {
    // Both of its callers catch and render the message beside their own
    // control, and SettingsPanel is the only view of the shared field -- so
    // recording it there too produced a message nothing displayed, which then
    // outlived the component that was supposed to display it (navigate away
    // and back and the failure is rendered nowhere at all). The rejection is
    // the report; the shared field is for actions that have no local line.
    const setSetting = vi.fn(async () => {
      throw new Error("disk full");
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    await expect(
      useSettingsStore
        .getState()
        .saveTtsSettings({ supertonicVoiceStyle: "F3" }),
    ).rejects.toThrow("disk full");

    expect(useSettingsStore.getState().error).toBeNull();
  });

  it("saveTtsSettings does not change a value until its write has landed", async () => {
    // Playback keys its engine on these rows. Setting them before the write
    // is known to have succeeded means an auto-advance sentence boundary
    // inside that window rebuilds on a voice that is not committed: the
    // reader hears -- and on Fish pays for -- a sentence in a voice change
    // that is then reverted with a failure banner. Applying only what landed
    // removes the window instead of compensating for it, and leaves nothing
    // to revert.
    let land = () => {};
    const landed = new Promise<void>((resolve) => {
      land = resolve;
    });
    const setSetting = vi.fn(async () => {
      await landed;
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    useSettingsStore.setState({ supertonicVoiceStyle: "M1" });
    const saving = useSettingsStore
      .getState()
      .saveTtsSettings({ supertonicVoiceStyle: "F3" });

    expect(useSettingsStore.getState().supertonicVoiceStyle).toBe("M1");

    land();
    await saving;

    expect(useSettingsStore.getState().supertonicVoiceStyle).toBe("F3");
  });

  it("saveTtsSettings writes only the rows its patch names", async () => {
    // Writing all four rows on every call is what lets two saves on one
    // screen clobber each other -- FishAudioSettings renders inside
    // SettingsPanel, so both Save buttons are live at once. A patch of
    // {fishVoiceId} rewriting supertonic_voice_style with the value it read
    // at its own start undoes a style the other save committed in between,
    // and reports success for a row the caller never asked to change.
    const setSetting = vi.fn(async (_key: string, _value: unknown) => undefined);
    const useSettingsStore = await loadSettingsStore(setSetting);

    useSettingsStore.setState({ supertonicVoiceStyle: "M1" });
    await useSettingsStore.getState().saveTtsSettings({ fishVoiceId: "v-2" });

    expect(setSetting.mock.calls.map((call) => call[0])).toEqual([
      "fish_voice_id",
    ]);
  });

  it("saveTtsSettings names which settings failed, so a partial save is not read as a total one", async () => {
    // The rows are written independently, so some can land while others fail.
    // Reporting a bare "disk full" tells the reader nothing saved, while the
    // language they picked is on disk and every book will be read in it from
    // now on -- including after the next launch.
    const setSetting = vi.fn(async (key: string) => {
      if (key === "supertonic_voice_style") {
        throw new Error("disk full");
      }
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    await expect(
      useSettingsStore.getState().saveTtsSettings({
        supertonicVoiceStyle: "F3",
        supertonicLanguage: "ko",
      }),
    ).rejects.toThrow("Voice style: disk full");

    // The row that did land stays landed.
    expect(useSettingsStore.getState().supertonicLanguage).toBe("ko");
  });

  it("saveTtsSettings rethrows the backend failure untouched when one row can fail", async () => {
    // A row label disambiguates *which* row failed. With one row in the patch
    // there is nothing to disambiguate, and wrapping costs real information:
    // the reader reads "Fish Audio voice: Fish Audio rejected your API key"
    // under a control already labelled that, and `asAppError` -- which
    // `fishFailureMessage` in player.ts switches on -- stops finding a `kind`.
    const reason = {
      kind: "payment_required",
      message: "out of credit",
      retryable: false,
    };
    const setSetting = vi.fn(async () => {
      throw reason;
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    const failure = await useSettingsStore
      .getState()
      .saveTtsSettings({ fishVoiceId: "voice-2" })
      .catch((error: unknown) => error);

    expect(failure).toBe(reason);
  });

  it("saveTtsSettings does nothing at all for a patch that names no rows", async () => {
    // It used to clear the shared banner and resolve, reporting a save that
    // wrote nothing as a success -- and wiping the message from whatever
    // really did fail.
    const setSetting = vi.fn(async (_key: string, _value: unknown) => undefined);
    const useSettingsStore = await loadSettingsStore(setSetting);

    useSettingsStore.setState({ error: "an earlier failure" });
    await useSettingsStore.getState().saveTtsSettings({});

    expect(setSetting).not.toHaveBeenCalled();
    expect(useSettingsStore.getState().error).toBe("an earlier failure");
  });

  it("saveTtsSettings states one shared cause once, not once per row", async () => {
    // SettingsPanel always sends both Supertonic rows, and the failures that
    // actually happen -- locked database, read-only disk -- take both. Naming
    // rows earns its keep when it tells the reader what *did* land; when
    // nothing did and the cause is one, it just stutters. Rethrowing the
    // original also keeps the backend error intact for anything that
    // classifies rejections.
    const reason = {
      kind: "database",
      message: "database is locked",
      retryable: true,
    };
    const setSetting = vi.fn(async () => {
      throw reason;
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    const failure = await useSettingsStore
      .getState()
      .saveTtsSettings({ supertonicVoiceStyle: "F3", supertonicLanguage: "ko" })
      .catch((error: unknown) => error);

    expect(failure).toBe(reason);
  });

  it("saveTtsSettings reports every distinct reason, not just the first", async () => {
    // Two rows can fail for separately actionable reasons. Surfacing only the
    // first hides the other, and the reader retries against a cause they were
    // never told about.
    const setSetting = vi.fn(async (key: string) => {
      throw new Error(
        key === "supertonic_voice_style" ? "disk full" : "database is locked",
      );
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    const failure = await useSettingsStore
      .getState()
      .saveTtsSettings({ supertonicVoiceStyle: "F3", supertonicLanguage: "ko" })
      .catch((error: unknown) => error);

    expect((failure as Error).message).toContain("disk full");
    expect((failure as Error).message).toContain("database is locked");
  });

  it("saveTtsSettings applies only the rows whose write actually landed", async () => {
    // The four writes are independent and go out together, so `Promise.all`
    // rejecting on one says nothing about the other three -- they still
    // commit. Applying all or none would leave the store disagreeing with the
    // DB one way or the other: playback speaking in a voice that was written,
    // or in one that was not, with the next launch silently flipping to
    // whatever the rows really hold.
    const setSetting = vi.fn(async (key: string) => {
      if (key === "fish_voice_id") {
        throw new Error("disk full");
      }
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    useSettingsStore.setState({
      supertonicVoiceStyle: "M1",
      fishVoiceId: "voice-1",
    });

    await expect(
      useSettingsStore.getState().saveTtsSettings({
        supertonicVoiceStyle: "F3",
        fishVoiceId: "voice-2",
      }),
    ).rejects.toThrow("disk full");

    // Written to the DB, so it must stand.
    expect(useSettingsStore.getState().supertonicVoiceStyle).toBe("F3");
    // Never written, so it must not.
    expect(useSettingsStore.getState().fishVoiceId).toBe("voice-1");
  });

  it("setTtsProvider does not change the provider until its write has landed", async () => {
    // `ttsProvider` is the first component of `engineKey`, which playback
    // reads at every sentence boundary. Setting it before the write resolves
    // lets an auto-advance inside that window build an engine for a provider
    // that was never saved -- and when that provider is Fish, synthesize over
    // the network and bill the reader for a selection that then reverts.
    let land = () => {};
    const landed = new Promise<void>((resolve) => {
      land = resolve;
    });
    const setSetting = vi.fn(async () => {
      await landed;
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    const switching = useSettingsStore.getState().setTtsProvider("fish");

    expect(useSettingsStore.getState().ttsProvider).toBe("supertonic");

    land();
    await switching;

    expect(useSettingsStore.getState().ttsProvider).toBe("fish");
  });

  it("setTtsProvider settles on the last provider clicked, not the last write to resolve", async () => {
    // SettingsPanel fires this unawaited, so two clicks inside one write's
    // window are two concurrent invokes. Left to race, the store keeps
    // whichever *resolved* last while SQLite keeps whichever *committed*
    // last -- the panel showing one provider while the row holds the other,
    // which is the divergence apply-after-write exists to prevent.
    const order: string[] = [];
    const setSetting = vi.fn(async (_key: string, value: unknown) => {
      order.push(String(value));
      // The first write is the slow one, so an unserialized implementation
      // resolves them out of order.
      if (order.length === 1) {
        await new Promise((resolve) => setTimeout(resolve, 20));
      }
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    const first = useSettingsStore.getState().setTtsProvider("fish");
    const second = useSettingsStore.getState().setTtsProvider("supertonic");
    await Promise.all([first, second]);

    expect(order).toEqual(["fish", "supertonic"]);
    expect(useSettingsStore.getState().ttsProvider).toBe("supertonic");
  });

  it("setTtsProvider leaves the provider alone when the persist fails", async () => {
    const setSetting = vi.fn(async () => {
      throw new Error("disk full");
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    // Seeded to "fish" -- different from the "supertonic" being set below --
    // so a no-op cannot pass this assertion by coincidence.
    useSettingsStore.setState({ ttsProvider: "fish" });

    await expect(
      useSettingsStore.getState().setTtsProvider("supertonic"),
    ).rejects.toThrow("disk full");

    expect(useSettingsStore.getState().ttsProvider).toBe("fish");
  });

  it("setTheme reverts to the previously stored theme when the persist fails", async () => {
    const setSetting = vi.fn(async () => {
      throw new Error("disk full");
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    // Seeded to "dark" -- different from the "light" being set below -- for
    // the same reason as the setTtsProvider revert test above.
    useSettingsStore.setState({ theme: "dark" });

    await expect(
      useSettingsStore.getState().setTheme("light"),
    ).rejects.toThrow("disk full");

    expect(useSettingsStore.getState().theme).toBe("dark");
  });
});
