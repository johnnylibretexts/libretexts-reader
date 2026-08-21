import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FishKeyStatus } from "../../lib/tauri";
import type { SpeechVoice } from "../../lib/speech/types";

const getFishKeyStatus = vi.fn();
const setFishApiKey = vi.fn();
const listFishVoices = vi.fn();
const clearFishApiKey = vi.fn();
const setSetting = vi.fn(async (_key: string, _value: unknown) => undefined);

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
    setSetting: (key: string, value: unknown) => setSetting(key, value),
    getAllSettings: vi.fn(async () => ({})),
  },
}));

const { FishAudioSettings } = await import("./FishAudioSettings");
const { useSettingsStore } = await import("../../stores/settings");

const SAVED_KEY: FishKeyStatus = { present: true, valid: true, credit: 10 };

function voice(id: string, name: string): SpeechVoice {
  return { id, name, ready: true };
}

describe("FishAudioSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getFishKeyStatus.mockResolvedValue(SAVED_KEY);
    clearFishApiKey.mockResolvedValue(undefined);
    setSetting.mockResolvedValue(undefined);
    useSettingsStore.setState({ hydrateFailed: false, fishVoiceId: null });
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

  it("keeps a voice on screen while a pasted id is being saved", async () => {
    // `saveTtsSettings` applies only once the write lands, so the stored id
    // is the one that has not caught up yet. The pending-pick state added for
    // that has to drive every derivation, not just the select's value: with
    // `isPastedVoice` still coming from the stored id, the pasted <option>
    // was not rendered at all and the control it was meant to steady went
    // blank -- "Choose a voice", while a voice was very much being chosen.
    listFishVoices.mockResolvedValue([voice("own-1", "My Own Voice")]);
    useSettingsStore.setState({ fishVoiceId: "own-1" });
    let land = () => {};
    const landed = new Promise<void>((resolve) => {
      land = resolve;
    });
    setSetting.mockImplementation(async () => {
      await landed;
    });
    const user = userEvent.setup();
    render(<FishAudioSettings />);

    const select = await screen.findByLabelText("Your voices");
    expect(select).toHaveValue("own-1");

    await user.type(screen.getByPlaceholderText("Voice id"), "public-42");
    await user.click(screen.getByRole("button", { name: /Use voice/ }));

    await waitFor(() =>
      expect(
        screen.getByRole("option", { name: /pasted voice id/i }),
      ).toBeInTheDocument(),
    );
    expect(select).not.toHaveValue("");

    land();
  });
  it("confirms a save started from the voices dropdown", async () => {
    // `savedFrom === "list"` was set and then rendered nowhere: the only
    // reader of it is the pasted-id button, which checks for "pasted". So a
    // dropdown save spun and then went silent -- the select snapped to the
    // new voice with nothing saying the write landed, on the one control
    // where the value changing is also what a *failed* save looks like.
    listFishVoices.mockResolvedValue([
      voice("own-1", "My Own Voice"),
      voice("own-2", "Another Voice"),
    ]);
    useSettingsStore.setState({ fishVoiceId: "own-1" });
    const user = userEvent.setup();
    render(<FishAudioSettings />);

    // The option, not just the select: `keyStatus` resolving renders the
    // dropdown one commit before the voices effect sets `loadingVoices`, so
    // there is a window where it is on screen holding "No voice models yet".
    await screen.findByRole("option", { name: "Another Voice" });
    await user.selectOptions(screen.getByLabelText("Your voices"), ["own-2"]);

    expect(await screen.findByText("Voice saved")).toBeInTheDocument();
  });

  it("keeps the dropdown confirmation off a pasted save", async () => {
    // The tick used to be shared state, which put it on whichever control
    // the reader had not used. Splitting `savedFrom` fixed that direction;
    // this holds the other one -- the pasted-id button has its own tick and
    // must not also light up the dropdown's line.
    listFishVoices.mockResolvedValue([voice("own-1", "My Own Voice")]);
    const user = userEvent.setup();
    render(<FishAudioSettings />);

    await screen.findByRole("option", { name: "My Own Voice" });
    await user.type(screen.getByPlaceholderText("Voice id"), "public-42");
    await user.click(screen.getByRole("button", { name: /Use voice/ }));

    await waitFor(() => expect(setSetting).toHaveBeenCalled());
    expect(screen.queryByText("Voice saved")).not.toBeInTheDocument();
  });
  it("announces a pasted save on the pasted control", async () => {
    // The button's tick is an `aria-hidden` icon and its name stays "Use
    // voice", so the save landed with nothing announced at all -- the same
    // silence the dropdown had, on the control that was supposed to be the
    // one already covered.
    listFishVoices.mockResolvedValue([voice("own-1", "My Own Voice")]);
    const user = userEvent.setup();
    render(<FishAudioSettings />);

    await screen.findByRole("option", { name: "My Own Voice" });
    await user.type(screen.getByPlaceholderText("Voice id"), "public-42");
    await user.click(screen.getByRole("button", { name: /Use voice/ }));

    expect(await screen.findByText("Pasted voice id saved")).toBeInTheDocument();
  });

  it("retires the tick with the key whose voices it confirmed", async () => {
    // The confirmation outlives the select it sits under: Remove inside the
    // 1.6s window swapped the dropdown for "Add an API key above to see your
    // saved voices." and left "Voice saved" underneath it, still confirming
    // a save against an account whose key is gone.
    listFishVoices.mockResolvedValue([
      voice("own-1", "My Own Voice"),
      voice("own-2", "Another Voice"),
    ]);
    useSettingsStore.setState({ fishVoiceId: "own-1" });
    const user = userEvent.setup();
    render(<FishAudioSettings />);

    await screen.findByRole("option", { name: "Another Voice" });
    await user.selectOptions(screen.getByLabelText("Your voices"), ["own-2"]);
    expect(await screen.findByText("Voice saved")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Remove" }));

    expect(
      await screen.findByText(/Add an API key above/),
    ).toBeInTheDocument();
    expect(screen.queryByText("Voice saved")).not.toBeInTheDocument();
  });
});
