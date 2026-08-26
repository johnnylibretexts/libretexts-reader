import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../../types/domain";

const listTranslationTargets = vi.fn(async (_sourceLang: string) => ["es", "fr"]);
const getTranslationModelStatus = vi.fn(
  async (
    _sourceLang: string,
    _targetLang: string,
  ): Promise<Domain.TranslationModelStatus> => ({
    downloaded: true,
    downloadedBytes: 495_887_877,
    totalBytes: 495_887_877,
    verified: true,
  }),
);
const ensureTranslationModelsDownloaded = vi.fn(
  async (_sourceLang: string, _targetLang: string) => "models",
);
const cancelTranslationModelDownload = vi.fn(async () => undefined);
const setSetting = vi.fn(async (_key: string, _value: unknown) => undefined);
let progressListener:
  | ((event: { payload: Domain.TranslationModelDownloadProgress }) => void)
  | null = null;

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    async (
      _event: string,
      listener: (event: {
        payload: Domain.TranslationModelDownloadProgress;
      }) => void,
    ) => {
      progressListener = listener;
      return () => undefined;
    },
  ),
}));

vi.mock("../../lib/tauri", () => ({
  api: {
    listTranslationTargets: (sourceLang: string) =>
      listTranslationTargets(sourceLang),
    getTranslationModelStatus: (sourceLang: string, targetLang: string) =>
      getTranslationModelStatus(sourceLang, targetLang),
    ensureTranslationModelsDownloaded: (
      sourceLang: string,
      targetLang: string,
    ) => ensureTranslationModelsDownloaded(sourceLang, targetLang),
    cancelTranslationModelDownload: () => cancelTranslationModelDownload(),
    setSetting: (key: string, value: unknown) => setSetting(key, value),
  },
  isTauriRuntime: () => true,
}));

const { ListenInControl } = await import("./ListenInControl");
const { usePlayerStore } = await import("../../stores/player");
const { useSettingsStore } = await import("../../stores/settings");
const { useTranslationStore } = await import("../../stores/translation");

const DOCUMENT: Domain.Document = {
  id: "doc-1",
  title: "Biology",
  sourceType: "libretexts",
  sourceMetadata: null,
  coverImagePath: null,
  license: null,
  attribution: null,
  wordCount: 100,
  sourceLanguage: "en",
  importedAt: "2026-01-01T00:00:00Z",
  lastOpenedAt: null,
  progress: 0,
};

describe("ListenInControl", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    progressListener = null;
    listTranslationTargets.mockResolvedValue(["es", "fr"]);
    getTranslationModelStatus.mockResolvedValue({
      downloaded: true,
      downloadedBytes: 495_887_877,
      totalBytes: 495_887_877,
      verified: true,
    });
    ensureTranslationModelsDownloaded.mockResolvedValue("models");
    cancelTranslationModelDownload.mockResolvedValue(undefined);
    setSetting.mockResolvedValue(undefined);
    usePlayerStore.setState({
      document: DOCUMENT,
      isPlaying: false,
      isBuffering: false,
      play: vi.fn(async () => undefined),
      pause: vi.fn(),
    });
    useSettingsStore.setState({
      hydrated: true,
      hydrateFailed: false,
      translationTargetLang: null,
      supertonicLanguage: "en",
    });
    useTranslationStore.setState({
      sectionState: {
        status: "complete",
        done: 10,
        total: 10,
        fallbackCount: 0,
        sentenceCount: 10,
        error: null,
      },
    });
  });

  it("saves a ready language to the preference shared with Settings", async () => {
    const user = userEvent.setup();
    render(<ListenInControl />);

    await screen.findByRole("option", { name: "Spanish" });
    await user.selectOptions(screen.getByLabelText("Listen in"), "es");

    await waitFor(() =>
      expect(useSettingsStore.getState().translationTargetLang).toBe("es"),
    );
    expect(setSetting).toHaveBeenCalledWith("translation_target_lang", "es");
    expect(setSetting).toHaveBeenCalledWith("supertonic_language", "es");
    expect(screen.getByLabelText("Listen in")).toHaveValue("es");
  });

  it("requires size confirmation before downloading and playing", async () => {
    getTranslationModelStatus.mockResolvedValue({
      downloaded: false,
      downloadedBytes: 0,
      totalBytes: 495_887_877,
      verified: true,
    });
    const play = vi.fn(async () => undefined);
    usePlayerStore.setState({ play });
    const user = userEvent.setup();
    render(<ListenInControl />);

    await screen.findByRole("option", { name: "Spanish" });
    await user.selectOptions(screen.getByLabelText("Listen in"), "es");

    expect(
      await screen.findByRole("dialog", { name: "Download Spanish narration" }),
    ).toHaveTextContent("496 MB");
    expect(useSettingsStore.getState().translationTargetLang).toBeNull();
    expect(ensureTranslationModelsDownloaded).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Download & play" }));

    await waitFor(() =>
      expect(ensureTranslationModelsDownloaded).toHaveBeenCalledWith("en", "es"),
    );
    await waitFor(() => expect(play).toHaveBeenCalledTimes(1));
    expect(useSettingsStore.getState().translationTargetLang).toBe("es");
  });

  it("offers setup when Settings selected a language that is not downloaded", async () => {
    useSettingsStore.setState({ translationTargetLang: "es" });
    getTranslationModelStatus.mockResolvedValue({
      downloaded: false,
      downloadedBytes: 0,
      totalBytes: 495_887_877,
      verified: true,
    });
    const user = userEvent.setup();
    render(<ListenInControl />);

    const setup = await screen.findByRole("button", {
      name: "Download · 496 MB",
    });
    expect(screen.getByLabelText("Listen in")).toHaveValue("es");
    await user.click(setup);

    expect(
      screen.getByRole("dialog", { name: "Download Spanish narration" }),
    ).toBeInTheDocument();
  });

  it("shows download progress and lets the reader cancel", async () => {
    getTranslationModelStatus.mockResolvedValue({
      downloaded: false,
      downloadedBytes: 0,
      totalBytes: 495_887_877,
      verified: true,
    });
    ensureTranslationModelsDownloaded.mockReturnValue(new Promise(() => {}));
    const user = userEvent.setup();
    render(<ListenInControl />);

    await screen.findByRole("option", { name: "Spanish" });
    await user.selectOptions(screen.getByLabelText("Listen in"), "es");
    await user.click(
      await screen.findByRole("button", { name: "Download & play" }),
    );

    await waitFor(() => expect(progressListener).not.toBeNull());
    progressListener?.({
      payload: {
        sourceLang: "en",
        targetLang: "es",
        pair: "en-es",
        file: "model.bin",
        downloaded: 247_943_939,
        total: 495_887_877,
      },
    });

    expect(await screen.findByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "50",
    );
    expect(screen.getByText("248 MB of 496 MB")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Cancel download" }));
    expect(cancelTranslationModelDownload).toHaveBeenCalledTimes(1);
  });
});
