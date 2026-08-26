import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../../types/domain";
import type { TranslationSectionState } from "../../stores/translation";

vi.mock("./ReaderHeader", () => ({ ReaderHeader: () => <div>Reader header</div> }));
vi.mock("./ParagraphView", () => ({ ParagraphView: () => <div>Paragraph</div> }));
vi.mock("./SupertonicChapterExport", () => ({
  SupertonicChapterExport: () => <div>Chapter export</div>,
}));
vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => path,
  invoke: vi.fn(),
}));

const { Reader } = await import("./Reader");
const { usePlayerStore } = await import("../../stores/player");
const { useSettingsStore } = await import("../../stores/settings");
const { useTranslationStore } = await import("../../stores/translation");

const DOCUMENT: Domain.Document = {
  id: "doc-1",
  title: "Biology",
  sourceType: "openstax",
  sourceMetadata: null,
  coverImagePath: null,
  license: null,
  attribution: null,
  wordCount: 312,
  sourceLanguage: "en",
  importedAt: "2026-01-01T00:00:00Z",
  lastOpenedAt: null,
  progress: 0,
};

const COMPLETE: TranslationSectionState = {
  status: "complete",
  done: 312,
  total: 312,
  fallbackCount: 9,
  sentenceCount: 312,
  error: null,
};

beforeEach(() => {
  usePlayerStore.setState({
    document: DOCUMENT,
    sections: [
      {
        id: "sec-1",
        documentId: "doc-1",
        ordinal: 0,
        title: "Chapter One",
        wordCount: 312,
      },
    ],
    currentSectionIndex: 0,
    paragraphs: [],
    sectionImages: [],
    loading: false,
    error: null,
  });
  useSettingsStore.setState({ translationTargetLang: "es" });
  useTranslationStore.setState({ sectionState: COMPLETE });
});

afterEach(() => {
  usePlayerStore.setState({ document: null, sections: [], paragraphs: [] });
});

describe("translation status", () => {
  it("says how many sentences fell back to the original language", async () => {
    render(<Reader documentId="doc-1" />);
    expect(
      await screen.findByText(/9 of 312 sentences are read in English/),
    ).toBeInTheDocument();
  });

  it("says nothing when every sentence translated", () => {
    useTranslationStore.setState({
      sectionState: { ...COMPLETE, fallbackCount: 0 },
    });
    render(<Reader documentId="doc-1" />);
    expect(screen.queryByText(/read in English/)).not.toBeInTheDocument();
  });

  it("shows readable translation progress and a stopping state", async () => {
    const cancel = vi.fn(async () => undefined);
    useTranslationStore.setState({
      sectionState: { ...COMPLETE, status: "running", done: 40 },
      cancel,
    });
    render(<Reader documentId="doc-1" />);

    expect(screen.getByText("Preparing Spanish narration")).toBeInTheDocument();
    expect(screen.getByText("40 of 312 sentences")).toBeInTheDocument();
    expect(screen.getByText("13%")).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuetext",
      "40 of 312 sentences, 13 percent",
    );
    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "40",
    );

    await userEvent.click(screen.getByRole("button", { name: /Cancel/ }));
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Stopping…" })).toBeDisabled();
  });
});

it("does not put a second language setup card in the reading surface", () => {
  render(<Reader documentId="doc-1" />);

  expect(screen.queryByText("Book text")).not.toBeInTheDocument();
  expect(screen.queryByText("Narration")).not.toBeInTheDocument();
});
