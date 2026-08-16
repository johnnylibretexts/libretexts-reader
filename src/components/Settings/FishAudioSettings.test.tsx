import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FishKeyStatus } from "../../lib/tauri";
import type { SpeechVoice } from "../../lib/speech/types";

const getFishKeyStatus = vi.fn();
const setFishApiKey = vi.fn();
const listFishVoices = vi.fn();
const clearFishApiKey = vi.fn();

vi.mock("../../lib/tauri", () => ({
  isTauriRuntime: () => true,
  api: {
    get getFishKeyStatus() {
      return getFishKeyStatus;
    },
    get setFishApiKey() {
      return setFishApiKey;
    },
    get listFishVoices() {
      return listFishVoices;
    },
    get clearFishApiKey() {
      return clearFishApiKey;
    },
  },
}));

const { FishAudioSettings } = await import("./FishAudioSettings");

const SAVED_KEY: FishKeyStatus = { present: true, valid: true, credit: 10 };

function voice(id: string, name: string): SpeechVoice {
  return { id, name, ready: true };
}

describe("FishAudioSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getFishKeyStatus.mockResolvedValue(SAVED_KEY);
    clearFishApiKey.mockResolvedValue(undefined);
  });

  it("refetches the voice list when one API key is replaced with another", async () => {
    // Both statuses have present: true. An effect keyed on the boolean does
    // not re-run, so account A's voices stay listed -- and picking one
    // persists a reference_id account B does not own, which 404s on the first
    // synthesis into "Choose a Fish Audio voice in Settings" while a voice is
    // very much chosen.
    listFishVoices
      .mockResolvedValueOnce([voice("voice-a", "Voice From Account A")])
      .mockResolvedValueOnce([voice("voice-b", "Voice From Account B")]);
    setFishApiKey.mockResolvedValue({
      present: true,
      valid: true,
      credit: 5,
    } satisfies FishKeyStatus);

    render(<FishAudioSettings />);

    expect(
      await screen.findByRole("option", { name: "Voice From Account A" }),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Replace" }));
    await userEvent.type(
      screen.getByPlaceholderText("Paste your Fish Audio API key"),
      "sk-the-second-key",
    );
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(
      await screen.findByRole("option", { name: "Voice From Account B" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("option", { name: "Voice From Account A" }),
    ).not.toBeInTheDocument();
    expect(listFishVoices).toHaveBeenCalledTimes(2);
  });

  it("does not list voices when no key is stored", async () => {
    // The mirror of the case above: `present: false` must clear the list, not
    // merely stop refreshing it.
    getFishKeyStatus.mockResolvedValue({
      present: false,
      valid: null,
      credit: null,
    } satisfies FishKeyStatus);

    render(<FishAudioSettings />);

    await waitFor(() => expect(getFishKeyStatus).toHaveBeenCalled());
    expect(listFishVoices).not.toHaveBeenCalled();
  });
});
