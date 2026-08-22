import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const getSupertonicModelStatus = vi.fn(async () => ({
  downloaded: false,
  directory: "/models",
  downloadedBytes: 0,
  totalBytes: 401_276_744,
  missingFiles: ["model.onnx"],
}));

vi.mock("../../lib/tauri", () => ({
  api: {
    getSupertonicModelStatus: () => getSupertonicModelStatus(),
  },
}));

const { EmptyState } = await import("./EmptyState");
const { useSettingsStore } = await import("../../stores/settings");

beforeEach(() => {
  useSettingsStore.setState({ ttsProvider: "supertonic", hydrated: true });
});

afterEach(() => {
  getSupertonicModelStatus.mockClear();
  getSupertonicModelStatus.mockResolvedValue({
    downloaded: false,
    directory: "/models",
    downloadedBytes: 0,
    totalBytes: 401_276_744,
    missingFiles: ["model.onnx"],
  });
});

describe("warning about the one-time voice download", () => {
  it("says a download is coming, and how big, before the reader ever presses Play", async () => {
    // #52's second half: nothing anywhere told the reader that their first
    // Play would pull ~383MB. They found out by watching the app appear to
    // freeze.
    render(<EmptyState />);

    expect(
      await screen.findByText(/383 MB/, { exact: false }),
    ).toBeInTheDocument();
  });

  it("says nothing once the voice is already on disk", async () => {
    // There is no download to warn about, and a warning that fires anyway
    // teaches the reader to ignore the next one.
    getSupertonicModelStatus.mockResolvedValue({
      downloaded: true,
      directory: "/models",
      downloadedBytes: 401_276_744,
      totalBytes: 401_276_744,
      missingFiles: [],
    });

    render(<EmptyState />);

    await waitFor(() => expect(getSupertonicModelStatus).toHaveBeenCalled());
    expect(screen.queryByText(/383 MB/)).not.toBeInTheDocument();
  });

  it("says nothing to a reader whose voice is the cloud one", async () => {
    // Fish downloads no model at all, so this warning would describe
    // something that is never going to happen to them.
    useSettingsStore.setState({ ttsProvider: "fish", hydrated: true });

    render(<EmptyState />);

    expect(screen.queryByText(/383 MB/)).not.toBeInTheDocument();
    expect(getSupertonicModelStatus).not.toHaveBeenCalled();
  });

  it("still explains how to get started", async () => {
    render(<EmptyState />);

    expect(screen.getByText(/importing a book from OpenStax/i)).toBeInTheDocument();
  });
});
