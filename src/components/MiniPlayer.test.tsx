import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../types/domain";

const { MiniPlayer } = await import("./MiniPlayer");
const { usePlayerStore } = await import("../stores/player");
const { useSettingsStore } = await import("../stores/settings");

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

const DOWNLOADING = {
  isBuffering: true,
  bufferingMessage: "Downloading the on-device voice (one time)",
  modelDownload: { downloadedBytes: 164_000_000, totalBytes: 401_276_744 },
};

function showMiniPlayer(
  state: Partial<ReturnType<typeof usePlayerStore.getState>>,
) {
  usePlayerStore.setState({ document: DOCUMENT, sections: [], ...state });
  render(<MiniPlayer onClose={() => {}} />);
}

afterEach(() => {
  usePlayerStore.setState({
    document: null,
    isBuffering: false,
    bufferingMessage: "",
    modelDownload: null,
    positionError: null,
    error: null,
  });
  useSettingsStore.setState({ ttsProvider: "supertonic" });
  vi.restoreAllMocks();
});

describe("pausing an engine that bills", () => {
  it("keeps Pause reachable while a billing engine buffers", async () => {
    // Buffering is exactly when a Fish reader most needs Pause: the burst of
    // billed requests is in flight, and Pause is what stops the queue. A
    // disabled Pause makes "stop spending" literally unclickable.
    useSettingsStore.setState({ ttsProvider: "fish" });
    showMiniPlayer({
      isPlaying: true,
      isBuffering: true,
      bufferingMessage: "Buffering Fish Audio audio",
      modelDownload: null,
    });

    expect(screen.getByRole("button", { name: /^pause$/i })).toBeEnabled();
  });
});

describe("the one-time voice download in the mini player", () => {
  it("shows how far it has actually got", async () => {
    // The mini player is the surface a reader watches while the reader pane
    // scrolls, so it cannot be the one place the download is still invisible.
    showMiniPlayer(DOWNLOADING);

    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "41",
    );
    expect(screen.getByText(/156 MB of 383 MB/)).toBeInTheDocument();
  });

  it("keeps Play reachable while the voice downloads", async () => {
    showMiniPlayer(DOWNLOADING);

    expect(screen.getByRole("button", { name: /^(play|pause)$/i })).toBeEnabled();
  });

  it("stops the download when the reader cancels it", async () => {
    const cancelModelDownload = vi.fn();
    showMiniPlayer({ ...DOWNLOADING, cancelModelDownload });

    await userEvent.click(screen.getByRole("button", { name: /cancel/i }));

    expect(cancelModelDownload).toHaveBeenCalledTimes(1);
  });

  it("shows no bar and no Cancel for ordinary buffering", async () => {
    showMiniPlayer({
      isBuffering: true,
      bufferingMessage: "Buffering Supertonic audio",
      modelDownload: null,
    });

    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /cancel/i }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^(play|pause)$/i })).toBeDisabled();
  });
});

describe("when the reader's place is not being saved", () => {
  it("says so on the surface the reader watches while listening", async () => {
    showMiniPlayer({
      positionError:
        "Your place in this book is not being saved. (database is locked)",
    });

    expect(screen.getByRole("status")).toHaveTextContent(/not being saved/i);
  });

  it("does not dress a bookkeeping failure up as a playback failure", async () => {
    // The red banner is where a synthesis failure lives, and it is what gates
    // "Switch to Supertonic". A cursor that cannot be written has nothing to
    // do with either, and offering that button here would be nonsense.
    showMiniPlayer({
      positionError: "Your place in this book is not being saved.",
      error: null,
      canSwitchToSupertonic: false,
    });

    expect(
      screen.queryByRole("button", { name: /switch to supertonic/i }),
    ).not.toBeInTheDocument();
  });
});
