import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SpeechEngine, SpeechEngineSettings } from "../../lib/speech";

const createSpeechEngine = vi.fn((_settings: SpeechEngineSettings) => engine);
const setSetting = vi.fn(async (_key: string, _value: unknown) => undefined);
const getFishKeyStatus = vi.fn(async () => ({ present: false, valid: false }));
const getAllSettings = vi.fn(async (): Promise<Record<string, unknown>> => ({}));

vi.mock("../../lib/speech", async () => ({
  ...(await vi.importActual<typeof import("../../lib/speech")>(
    "../../lib/speech",
  )),
  createSpeechEngine: (settings: SpeechEngineSettings) =>
    createSpeechEngine(settings),
}));

// isTauriRuntime false keeps the model-status effect and the download-progress
// `listen` subscription out of the way: neither is what this file is about.
vi.mock("../../lib/tauri", () => ({
  isTauriRuntime: () => false,
  api: {
    getSupertonicModelStatus: vi.fn(async () => ({ downloaded: true })),
    ensureSupertonicModelDownloaded: vi.fn(async () => ""),
    setSetting: (key: string, value: unknown) => setSetting(key, value),
    getFishKeyStatus: () => getFishKeyStatus(),
    listFishVoices: vi.fn(async () => []),
    getAllSettings: () => getAllSettings(),
  },
}));

const engine: SpeechEngine = {
  id: "supertonic",
  defaultVoice: "M1",
  synthesize: vi.fn(async () => new Blob(["audio"], { type: "audio/wav" })),
  ensureReady: vi.fn(async () => undefined),
  listVoices: vi.fn(async () => []),
};

const { SettingsPanel } = await import("./SettingsPanel");
const { useSettingsStore } = await import("../../stores/settings");

describe("SettingsPanel voice test", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setSetting.mockResolvedValue(undefined);
    getFishKeyStatus.mockResolvedValue({ present: false, valid: false });
    // `clearAllMocks` resets calls, not implementations, and the store is
    // shared across tests in this file -- so both have to be put back or a
    // hydrate failure seeded by one test leaks into the next.
    getAllSettings.mockResolvedValue({});
    useSettingsStore.setState({
      hydrated: true,
      hydrateFailed: false,
      loading: false,
      error: null,
      ttsProvider: "supertonic",
      supertonicVoiceStyle: "M1",
      supertonicLanguage: "en",
      fishVoiceId: null,
    });
  });

  it("tests the voice style the reader has selected, not the one last saved", async () => {
    // Test speaks through `createSpeechEngine`, which now takes the Supertonic
    // voice style as engine configuration rather than per-call. Handing it the
    // saved settings row would make Test preview the previous voice whenever a
    // reader auditions a style before saving -- the same class of mismatch as
    // the playback bug this pairs with, just in the opposite direction.
    // `language` was already read from pending local state here; the style has
    // to match it.
    const user = userEvent.setup();
    render(<SettingsPanel />);

    await user.selectOptions(screen.getByLabelText("Voice style"), "F3");
    await user.click(
      screen.getByRole("button", { name: /Test Supertonic voice/ }),
    );

    await waitFor(() => expect(createSpeechEngine).toHaveBeenCalled());
    expect(createSpeechEngine.mock.calls[0][0]).toMatchObject({
      ttsProvider: "supertonic",
      supertonicVoiceStyle: "F3",
    });
  });

  it("still reports a later failure from a different control after a failed save", async () => {
    // A failed save must not swallow an unrelated failure: here it is
    // followed by a provider switch that fails too, and the reader has to be
    // told why the provider snapped back.
    // Distinct messages on purpose: both banners would otherwise match the
    // same text, and an assertion that the text appears "somewhere" passes on
    // the stale save banner while the provider failure is nowhere on screen.
    setSetting.mockImplementation(async (key: string) => {
      throw new Error(
        key === "tts_provider" ? "provider write failed" : "draft write failed",
      );
    });
    const user = userEvent.setup();
    render(<SettingsPanel />);

    await user.click(screen.getByRole("button", { name: /Save/ }));
    await screen.findByText(/draft write failed/);

    await user.click(screen.getByRole("button", { name: /Fish Audio/ }));

    await screen.findByText(/provider write failed/);
    expect(useSettingsStore.getState().ttsProvider).toBe("supertonic");
  });

  it("shows a failed Fish voice save once, not once per place that caught it", async () => {
    // Same duplication as the Supertonic save, from the sibling control: it
    // reaches `saveTtsSettings` too, which used to write the shared banner as
    // well as rejecting into FishAudioSettings' own voice-error line.
    getFishKeyStatus.mockResolvedValue({ present: true, valid: true });
    setSetting.mockRejectedValue(new Error("disk full"));
    const user = userEvent.setup();
    render(<SettingsPanel />);

    await user.type(await screen.findByPlaceholderText("Voice id"), "voice-42");
    await user.click(screen.getByRole("button", { name: /Use voice/ }));

    // Singular: `findByText` throws on more than one match.
    await screen.findByText(/disk full/);
  });

  it("reports a provider failure worded exactly like a Fish voice failure already on screen", async () => {
    // These two once both wrote the shared field, and the banner tried to
    // tell them apart by comparing the *message* -- which cannot, so the
    // provider snapped back with no new red text anywhere. Only the provider
    // switch reaches that field now, and this is what keeps it that way.
    getFishKeyStatus.mockResolvedValue({ present: true, valid: true });
    setSetting.mockRejectedValue(new Error("database is locked"));
    const user = userEvent.setup();
    render(<SettingsPanel />);

    await user.type(await screen.findByPlaceholderText("Voice id"), "voice-42");
    await user.click(screen.getByRole("button", { name: /Use voice/ }));
    await waitFor(() =>
      expect(screen.getAllByText(/database is locked/)).toHaveLength(1),
    );

    await user.click(screen.getByRole("button", { name: /Fish Audio/ }));

    // The provider failure has no local line of its own, so it has to show up
    // in the shared banner -- alongside, not instead of, the voice error.
    await waitFor(() =>
      expect(screen.getAllByText(/database is locked/)).toHaveLength(2),
    );
    expect(useSettingsStore.getState().ttsProvider).toBe("supertonic");
  });

  it("shows the provider switch as pending until its write lands", async () => {
    // `setTtsProvider` applies only once the write commits, so that playback
    // never builds an engine for a provider that did not stick. Nothing in
    // the picker moves until then -- so on a slow write it looked inert and
    // invited a second click, which only queued another write behind the
    // first.
    let land = () => {};
    const landed = new Promise<void>((resolve) => {
      land = resolve;
    });
    setSetting.mockImplementation(async () => {
      await landed;
    });
    const user = userEvent.setup();
    render(<SettingsPanel />);

    // Selected by `aria-pressed`, which only the two provider buttons carry:
    // matching on the label would also catch "Test <provider> voice".
    const fish = screen.getByRole("button", { pressed: false });
    const supertonic = screen.getByRole("button", { pressed: true });
    await user.click(fish);

    expect(
      await screen.findByLabelText("Switching to Fish Audio"),
    ).toBeVisible();
    expect(fish).toBeDisabled();
    // The other one stays live on purpose: a write that never settles would
    // otherwise leave the picker dead with no way back, and the write queue
    // already makes a second click safe.
    expect(supertonic).toBeEnabled();

    land();

    await waitFor(() => expect(fish).toBeEnabled());
    expect(useSettingsStore.getState().ttsProvider).toBe("fish");
  });

  it("will not save over the reader's rows when settings failed to load", async () => {
    // `hydrate` falls back to DEFAULT_SETTINGS and still reports `hydrated`,
    // so the panel renders "Male 1" as though it were saved. Before this
    // change that was inaudible, because playback ignored the row. Now Save
    // would write those defaults over rows the reader really has -- their
    // voice and language replaced by ones they never chose, from a screen
    // that never showed them theirs.
    getAllSettings.mockRejectedValue(new Error("database is locked"));
    const { useSettingsStore: store } = await import("../../stores/settings");
    store.setState({ hydrated: false, loading: false });
    await store.getState().hydrate();

    const user = userEvent.setup();
    render(<SettingsPanel />);

    await screen.findByText(/database is locked/);
    expect(screen.getByRole("button", { name: /Save/ })).toBeDisabled();
    // The picker too: it renders DEFAULT_SETTINGS, so a reader whose stored
    // provider is Fish sees Supertonic highlighted as though they had chosen
    // it. Any click there commits a provider this screen never showed them.
    expect(screen.getByRole("button", { pressed: true })).toBeDisabled();
    expect(screen.getByRole("button", { pressed: false })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: /Save/ }));
    expect(setSetting).not.toHaveBeenCalled();

    // The Voice style select stays editable while Save is off, so a reader
    // can line up their pick while waiting. A retry must not then replace it
    // at the very moment Save becomes usable.
    await user.selectOptions(screen.getByLabelText("Voice style"), "F5");

    // And the reader can get their real rows back without restarting.
    getAllSettings.mockResolvedValue({ supertonic_voice_style: "F3" });
    await user.click(screen.getByRole("button", { name: "Try again" }));

    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Save/ })).toBeEnabled(),
    );
    expect(screen.getByLabelText("Voice style")).toHaveValue("F5");
    // The language they did not touch still picks up the loaded row.
    expect(screen.getByLabelText("Language")).toHaveValue("en");
  });

  it("keeps the reader's pending selection when the save fails", async () => {
    // This panel used to re-sync its draft from the store on every change to
    // those rows, and a failed save used to move them (it set optimistically
    // and reverted) -- so the dropdown snapped back underneath the failure
    // banner and the reader had to re-pick the style before they could retry.
    // Both halves are gone now; this holds the outcome in place.
    setSetting.mockRejectedValue(new Error("disk full"));
    const user = userEvent.setup();
    render(<SettingsPanel />);

    const voiceStyle = screen.getByLabelText("Voice style");
    await user.selectOptions(voiceStyle, "F3");
    await user.click(screen.getByRole("button", { name: /Save/ }));

    // Singular on purpose: `saveTtsSettings` used to record the message in
    // the shared banner *and* reject with it, so the panel rendered the same
    // failure twice. `findByText` throws on more than one match, so this is
    // the assertion.
    await screen.findByText(/disk full/);
    expect(voiceStyle).toHaveValue("F3");
  });
});
