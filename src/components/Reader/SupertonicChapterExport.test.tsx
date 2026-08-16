import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SupertonicChapterEstimate } from "../../lib/tauri";
import type * as Domain from "../../types/domain";

const estimateSupertonicChapter = vi.fn();
const exportSupertonicChapterMp3 = vi.fn();
const getFishCredit = vi.fn();
const previewSupertonicTts = vi.fn();

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
  },
}));

const { SupertonicChapterExport } = await import("./SupertonicChapterExport");
const { usePlayerStore } = await import("../../stores/player");
const { useSettingsStore } = await import("../../stores/settings");

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
    seedReader();
    useSettingsStore.setState({
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
