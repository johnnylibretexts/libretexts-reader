import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../../types/domain";

const { ReaderHeader } = await import("./ReaderHeader");
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
  progress: 0,
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

/** The real numbers: 383 MB across 16 files, ~41% of the way through. */
const DOWNLOADED_BYTES = 164_000_000;
const TOTAL_BYTES = 401_276_744;

function showReader(state: Partial<ReturnType<typeof usePlayerStore.getState>>) {
  usePlayerStore.setState({
    document: DOCUMENT,
    sections: SECTIONS,
    currentSectionIndex: 0,
    ...state,
  });
  render(<ReaderHeader />);
}

afterEach(() => {
  usePlayerStore.setState({
    document: null,
    sections: [],
    isBuffering: false,
    bufferingMessage: "",
    modelDownload: null,
  });
  vi.restoreAllMocks();
});

describe("licence and attribution on the reading surface", () => {
  it("credits a book whose attribution is an author name", async () => {
    // Pressbooks stores an author in `attribution`; OpenStax, LibreTexts and
    // article all store a URL there. The field is polymorphic, so it cannot be
    // rendered one way -- an author is text, not a link.
    showReader({
      document: {
        ...DOCUMENT,
        license: "CC BY-NC-SA 4.0",
        attribution: "Craig DeLancey",
      },
    });

    expect(screen.getByText(/CC BY-NC-SA 4\.0/)).toBeInTheDocument();
    expect(screen.getByText(/Craig DeLancey/)).toBeInTheDocument();
    expect(
      screen.queryByRole("link", { name: /Craig DeLancey/ }),
    ).not.toBeInTheDocument();
  });

  it("links back to the source when the attribution is a URL", async () => {
    // CC BY 4.0 3(a)(1) asks for a link to the material where reasonable, and
    // for three of the four Sources this field already is one.
    showReader({
      document: {
        ...DOCUMENT,
        license: "CC BY 4.0",
        attribution: "https://openstax.org/books/biology-2e",
      },
    });

    const link = screen.getByRole("link", { name: /openstax\.org/ });
    expect(link).toHaveAttribute("href", "https://openstax.org/books/biology-2e");
  });

  it("says nothing at all when the source supplied neither", async () => {
    // A pasted-text import has no licence and no attribution. An empty field
    // or a bare separator would claim the app knows something it does not.
    showReader({ document: { ...DOCUMENT, license: null, attribution: null } });

    expect(screen.queryByText(/·/)).not.toBeInTheDocument();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
    // The header itself still renders -- asserted on the heading, since the
    // section title also appears as an <option> in the Section select.
    expect(screen.getByRole("heading", { name: "A Book" })).toBeInTheDocument();
  });

  it("shows the licence alone when there is no attribution", async () => {
    showReader({
      document: { ...DOCUMENT, license: "CC BY 4.0", attribution: null },
    });

    expect(screen.getByText(/CC BY 4\.0/)).toBeInTheDocument();
    expect(screen.queryByText(/·/)).not.toBeInTheDocument();
  });
});

describe("the one-time voice download in the reader", () => {
  it("shows how far it has actually got", async () => {
    // #52: an indeterminate spinner was the entire report on a ~383MB fetch
    // that takes minutes, so a working download was indistinguishable from a
    // hung app.
    showReader({
      isBuffering: true,
      bufferingMessage: "Downloading the on-device voice (one time)",
      modelDownload: {
        downloadedBytes: DOWNLOADED_BYTES,
        totalBytes: TOTAL_BYTES,
      },
    });

    const bar = screen.getByRole("progressbar");
    expect(bar).toHaveAttribute("aria-valuenow", "41");
    expect(screen.getByText(/156 MB of 383 MB/)).toBeInTheDocument();
  });

  it("keeps Play reachable while the voice downloads", async () => {
    // Every control bound to `isBuffering`, so the one button a reader
    // reaches for when the app looks stuck was the one they could not press.
    showReader({
      isBuffering: true,
      bufferingMessage: "Downloading the on-device voice (one time)",
      modelDownload: {
        downloadedBytes: DOWNLOADED_BYTES,
        totalBytes: TOTAL_BYTES,
      },
    });

    expect(screen.getByRole("button", { name: /^(play|pause)$/i })).toBeEnabled();
  });

  it("stops the download when the reader cancels it", async () => {
    const cancelModelDownload = vi.fn();
    showReader({
      isBuffering: true,
      bufferingMessage: "Downloading the on-device voice (one time)",
      modelDownload: {
        downloadedBytes: DOWNLOADED_BYTES,
        totalBytes: TOTAL_BYTES,
      },
      cancelModelDownload,
    });

    await userEvent.click(screen.getByRole("button", { name: /cancel/i }));

    expect(cancelModelDownload).toHaveBeenCalledTimes(1);
  });

  it("leaves ordinary buffering as the plain spinner it was", async () => {
    // Buffering a sentence is seconds, and there is nothing to cancel. The
    // bar and the Cancel belong to the download alone.
    showReader({
      isBuffering: true,
      bufferingMessage: "Buffering Supertonic audio",
      modelDownload: null,
    });

    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /cancel/i }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("Buffering Supertonic audio")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^(play|pause)$/i })).toBeDisabled();
  });
});

describe("pausing an engine that bills", () => {
  afterEach(() => {
    useSettingsStore.setState({ ttsProvider: "supertonic" });
  });

  it("keeps Pause reachable while a billing engine buffers", async () => {
    useSettingsStore.setState({ ttsProvider: "fish" });
    showReader({
      isPlaying: true,
      isBuffering: true,
      bufferingMessage: "Buffering Fish Audio audio",
      modelDownload: null,
    });

    expect(screen.getByRole("button", { name: /^pause$/i })).toBeEnabled();
  });
});
