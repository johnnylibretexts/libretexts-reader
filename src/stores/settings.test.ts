import { afterEach, describe, expect, it, vi } from "vitest";

/**
 * `setTheme`, `setTtsProvider`, and `saveTtsSettings` all follow the same
 * shape: on a failed persist they record the message into the shared
 * `error` field for the settings banner, then must also reject their own
 * returned promise. A caller awaiting one of these needs to detect its own
 * failure via an ordinary try/catch -- reading the shared `error` field
 * instead is unreliable, since a concurrent unrelated action can overwrite
 * or clear it before the read happens (see FishAudioSettings.tsx's
 * persistVoice, a real caller built around this).
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

describe("settings store persistence failures", () => {
  it("saveTtsSettings rejects and records the error when the persist fails", async () => {
    const setSetting = vi.fn(async () => {
      throw new Error("disk full");
    });
    const useSettingsStore = await loadSettingsStore(setSetting);

    // Seeded so a no-op implementation of the error field couldn't pass this
    // assertion by accident -- only a real write proves the failure path ran.
    useSettingsStore.setState({ error: "stale error from another action" });

    await expect(
      useSettingsStore.getState().saveTtsSettings({ supertonicLanguage: "en" }),
    ).rejects.toThrow("disk full");

    expect(useSettingsStore.getState().error).toBe("disk full");
  });

  it("saveTtsSettings clears a stale error and resolves when the persist succeeds", async () => {
    const setSetting = vi.fn(async () => undefined);
    const useSettingsStore = await loadSettingsStore(setSetting);

    // Seeded so the success path is proven to clear the field, not merely
    // leave an already-null field alone.
    useSettingsStore.setState({ error: "stale error from another action" });

    await expect(
      useSettingsStore.getState().saveTtsSettings({ supertonicLanguage: "en" }),
    ).resolves.toBeUndefined();

    expect(useSettingsStore.getState().error).toBeNull();
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
});
