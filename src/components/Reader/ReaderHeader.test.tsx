import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../../types/domain";

const { ReaderHeader } = await import("./ReaderHeader");
const { usePlayerStore } = await import("../../stores/player");

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
