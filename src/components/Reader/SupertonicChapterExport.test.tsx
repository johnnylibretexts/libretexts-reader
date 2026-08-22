import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SupertonicChapterEstimate } from "../../lib/tauri";
import type * as Domain from "../../types/domain";

const estimateSupertonicChapter = vi.fn();
const exportSupertonicChapterMp3 = vi.fn();
const getFishCredit = vi.fn();
const previewSupertonicTts = vi.fn();
const setSetting = vi.fn(async (_key: string, _value: unknown) => undefined);

vi.mock("../../lib/tauri", () => ({
  api: {
    get estimateSupertonicChapter() {
      return estimateSupertonicChapter;
    },
    get exportSupertonicChapterMp3() {
      return exportSupertonicChapterMp3;
    },
    get getFishCredit() {
      return getFishCredit;
    },
    get previewSupertonicTts() {
      return previewSupertonicTts;
    },
    setSetting: (key: string, value: unknown) => setSetting(key, value),
  },
}));

const { SupertonicChapterExport } = await import("./SupertonicChapterExport");
const { usePlayerStore } = await import("../../stores/player");
const { useSettingsStore } = await import("../../stores/settings");
const { useChapterExportStore } = await import("../../stores/chapterExport");

const DOCUMENT: Domain.Document = {
  id: "doc-1",
  title: "A Book",
  sourceType: "openstax",
  sourceMetadata: null,
  coverImagePath: null,
  license: null,
  attribution: null,
  wordCount: 8,
  importedAt: "2026-01-01T00:00:00Z",
  lastOpenedAt: null,
};

const SECTIONS: Domain.Section[] = [
  {
    id: "sec-1",
    documentId: "doc-1",
    ordinal: 0,
    title: "Chapter One",
    wordCount: 8,
  },
];

const PARAGRAPHS: Domain.Paragraph[] = [
  {
    id: "para-1",
    sectionId: "sec-1",
    ordinal: 0,
    text: "First sentence.",
    sentenceOffsets: [[0, 15]],
    sentenceSpeech: ["First sentence spoken."],
  },
];

function estimate(
  overrides: Partial<SupertonicChapterEstimate> = {},
): SupertonicChapterEstimate {
  return {
    wordCount: 8,
    estimatedSeconds: 60,
    chunkCount: 1,
    cached: true,
    outputPath: "/tmp/Chapter One.mp3",
    billableCharacters: 0,
    ...overrides,
  } as SupertonicChapterEstimate;
}

function seedReader() {
  usePlayerStore.setState({
    document: DOCUMENT,
    sections: SECTIONS,
    currentSectionIndex: 0,
    paragraphs: PARAGRAPHS,
  });
}

describe("SupertonicChapterExport", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setSetting.mockResolvedValue(undefined);
    seedReader();
    // The export drafts outlive an unmount on purpose -- that is the point of
    // the store -- so they also outlive a test. No test below currently
    // depends on this (checked: the file passes without it, in order and
    // shuffled), because every test that reads Voice either picks it or
    // asserts a value that survives anyway. It is here so the next test
    // written does not have to discover that, the same way `hydrateFailed` is
    // reset above.
    useChapterExportStore.getState().reset();
    useSettingsStore.setState({
      hydrated: true,
      // Reset explicitly: one test sets this true, and the store is shared
      // across the file, so leaving it would run every later test against a
      // failed load and cascade misleadingly.
      hydrateFailed: false,
      ttsProvider: "supertonic",
      fishVoiceId: null,
      supertonicVoiceStyle: "M1",
      supertonicLanguage: "en",
    });
    getFishCredit.mockResolvedValue(12.5);
    exportSupertonicChapterMp3.mockResolvedValue({
      outputPath: "/tmp/Chapter One.mp3",
      cached: false,
      byteLength: 10,
      estimate: estimate(),
    });
  });

  it("does not render until settings have loaded", async () => {
    // Its Voice and Language start from the app defaults and it persists
    // nothing, so a Preview or Generate clicked before `hydrate()` resolves
    // would quietly render "M1"/"en" instead of the reader's saved rows --
    // and nothing afterwards would say so, because Settings goes on showing
    // the real values. Not rendering at all is what keeps that unreachable;
    // the seeding effect alone only fixed what the dropdowns showed.
    estimateSupertonicChapter.mockResolvedValue(estimate());
    useSettingsStore.setState({ hydrated: false });
    const { container } = render(<SupertonicChapterExport />);

    expect(container).toBeEmptyDOMElement();
    // The render gate sits after the hooks, so it does not stop the effects:
    // without its own guard the estimate effect prices the chapter for the
    // built-in defaults, reaching the backend with the values the gate exists
    // to keep it from using.
    expect(estimateSupertonicChapter).not.toHaveBeenCalled();

    act(() => {
      useSettingsStore.setState({ hydrated: true, supertonicVoiceStyle: "F3" });
    });

    // And once it does render, it renders the reader's rows, not the
    // built-in defaults its useState initialisers captured pre-hydration.
    expect(await screen.findByLabelText("Voice")).toHaveValue("F3");

    // Every effect in one commit sees that commit's closures, so gating the
    // estimate on `settingsHydrated` alone still let it run once holding the
    // pre-hydration draft -- pricing the chapter for a voice the reader never
    // picked, which is the whole thing the gate is there to stop.
    await waitFor(() => expect(estimateSupertonicChapter).toHaveBeenCalled());
    expect(
      estimateSupertonicChapter.mock.calls.map(
        (call: unknown[]) => (call[0] as { voiceStyle: string }).voiceStyle,
      ),
    ).toEqual(["F3"]);
  });

  it("keeps an export voice picked while settings were still failing", async () => {
    // The panel renders during a failed load (with a notice saying the
    // dropdowns are defaults), so the reader can pick there. A retry from
    // Settings then flips `hydrateFailed`, which this panel re-seeds on --
    // and re-seeding unconditionally throws their pick away with no
    // indication, right before Generate, re-pricing the chapter as it goes.
    estimateSupertonicChapter.mockResolvedValue(estimate());
    useSettingsStore.setState({
      hydrateFailed: true,
      supertonicVoiceStyle: "M1",
    });
    const user = userEvent.setup();
    render(<SupertonicChapterExport />);

    await user.selectOptions(await screen.findByLabelText("Voice"), "F3");
    act(() => {
      useSettingsStore.setState({
        hydrateFailed: false,
        supertonicVoiceStyle: "M2",
      });
    });

    expect(screen.getByLabelText("Voice")).toHaveValue("F3");
  });

  it("remembers its export voice across a trip out of the Reader", async () => {
    // `AppShell` switch-renders routes, so stepping out to the Library
    // unmounts this panel outright. Voice lived in `useState` and the "has
    // the reader picked?" flag beside it in a `useRef`, so both reset on the
    // way back -- the seeding effect ran again and replaced the pick with the
    // app's reading voice, displaying it as though the reader chose it.
    estimateSupertonicChapter.mockResolvedValue(estimate());
    const user = userEvent.setup();
    const reader = render(<SupertonicChapterExport />);

    await user.selectOptions(await screen.findByLabelText("Voice"), "F3");
    reader.unmount();
    render(<SupertonicChapterExport />);

    expect(await screen.findByLabelText("Voice")).toHaveValue("F3");
  });

  it("remembers its export language across a trip out of the Reader", async () => {
    // Its own flag, not one shared with Voice: a single "chosen" flag would
    // freeze Language on whatever it held the moment Voice was touched.
    estimateSupertonicChapter.mockResolvedValue(estimate());
    const user = userEvent.setup();
    const reader = render(<SupertonicChapterExport />);

    await user.selectOptions(await screen.findByLabelText("Language"), "ko");
    reader.unmount();
    render(<SupertonicChapterExport />);

    expect(await screen.findByLabelText("Language")).toHaveValue("ko");
  });

  it("never prices the chapter for the drafts a retry is replacing", async () => {
    // A retry lands `hydrateFailed: false` and the reader's rows in one
    // `set`, so this commit already has the real `fishVoiceId` -- an estimate
    // dependency -- while Voice and Language still hold the pre-retry
    // defaults the seeding effect is only now queuing over. A boolean
    // `seeded` is true by then, so it gates nothing: the estimate fired once
    // for "M1" before re-firing for the reader's voice. Same shape as the
    // mount bug `seeded` was added for, one commit later.
    estimateSupertonicChapter.mockResolvedValue(estimate());
    useSettingsStore.setState({
      hydrateFailed: true,
      supertonicVoiceStyle: "M1",
      fishVoiceId: null,
    });
    render(<SupertonicChapterExport />);

    // The failure path prices for the defaults on purpose -- the panel is up
    // and usable. It is the retry that must not.
    await waitFor(() => expect(estimateSupertonicChapter).toHaveBeenCalled());
    estimateSupertonicChapter.mockClear();

    act(() => {
      useSettingsStore.setState({
        hydrateFailed: false,
        supertonicVoiceStyle: "F3",
        fishVoiceId: "fish-voice-9",
      });
    });

    await waitFor(() => expect(estimateSupertonicChapter).toHaveBeenCalled());
    expect(
      estimateSupertonicChapter.mock.calls.map(
        (call: unknown[]) => (call[0] as { voiceStyle: string }).voiceStyle,
      ),
    ).toEqual(["F3"]);
  });

  it("exports in the chosen voice without changing the app default", async () => {
    // Picking a voice for one MP3 is not the same as choosing what the app
    // reads aloud. This panel sits directly above the paragraphs in the
    // Reader, and playback now keys its engine on the shared row -- so
    // persisting here switched the narration of the chapter being listened
    // to, mid-chapter, and left Settings showing a voice the reader never
    // chose there. It also meant a failed settings write aborted a purely
    // local, network-free export that never needed the setting.
    estimateSupertonicChapter.mockResolvedValue(estimate());
    const user = userEvent.setup();
    render(<SupertonicChapterExport />);

    await user.selectOptions(await screen.findByLabelText("Voice"), "F3");
    await user.click(screen.getByRole("button", { name: /MP3/ }));

    await waitFor(() => expect(exportSupertonicChapterMp3).toHaveBeenCalled());
    expect(exportSupertonicChapterMp3.mock.calls[0][0]).toMatchObject({
      voiceStyle: "F3",
    });
    expect(setSetting).not.toHaveBeenCalled();
  });

  it("auditions a voice without committing it as the app default", async () => {
    // `preview()` already sends the draft style straight to
    // previewSupertonicTts, so persisting it was a pure side effect -- and
    // now that playback keys its engine on that row, the side effect
    // repointed the narration of the book the reader is listening to. This
    // panel sits directly above the paragraphs in the Reader, so auditioning
    // an export voice must not change what is being read aloud.
    estimateSupertonicChapter.mockResolvedValue(estimate());
    previewSupertonicTts.mockResolvedValue({ audio: [1], mimeType: "audio/wav" });
    const user = userEvent.setup();
    render(<SupertonicChapterExport />);

    await user.selectOptions(await screen.findByLabelText("Voice"), "F3");
    await user.click(screen.getByRole("button", { name: "Preview" }));

    await waitFor(() => expect(previewSupertonicTts).toHaveBeenCalled());
    // The audition itself still uses the draft...
    expect(previewSupertonicTts.mock.calls[0][0]).toMatchObject({
      voiceStyle: "F3",
    });
    // ...but nothing about it is written to the shared settings row.
    expect(setSetting).not.toHaveBeenCalled();
  });

  it("leaves the export buttons usable when the estimate fails to load", async () => {
    // A transient estimate failure used to disable both buttons with no way
    // back: the estimate effect only re-runs on a chapter, voice or provider
    // change, so the reader had to navigate away and return. Supertonic costs
    // nothing, so it never needed a price to proceed at all.
    estimateSupertonicChapter.mockRejectedValue(new Error("pool exhausted"));

    render(<SupertonicChapterExport />);

    await waitFor(() =>
      expect(estimateSupertonicChapter).toHaveBeenCalledTimes(1),
    );

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /Generate MP3/ }),
      ).toBeEnabled(),
    );
    expect(screen.getByRole("button", { name: "Regenerate" })).toBeEnabled();
  });

  it("does not leave a forced price on screen after a cancelled regenerate", async () => {
    // Regenerate re-fetches with force: true, which prices a full
    // re-synthesis even for a cached chapter. That number belongs to the one
    // request. Written into the shared estimate it outlived the cancel, so
    // the standing line quoted a billable count for a plain Generate that
    // would have been served from the cache for nothing.
    useSettingsStore.setState({ ttsProvider: "fish", fishVoiceId: "voice-1" });
    estimateSupertonicChapter.mockImplementation(
      ({ force }: { force?: boolean }) =>
        Promise.resolve(
          force
            ? estimate({ cached: false, billableCharacters: 4321 })
            : estimate({ cached: true, billableCharacters: 0 }),
        ),
    );

    render(<SupertonicChapterExport />);

    expect(await screen.findByText(/0 billable characters/)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Regenerate" }));
    expect(
      await screen.findByText(/4,321 characters/),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(
      await screen.findByText(/0 billable characters/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/4,321/)).not.toBeInTheDocument();
  });
});
