import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../types/domain";

const listen = vi.fn();
const translateSection = vi.fn();
const cancelSectionTranslation = vi.fn(async () => undefined);
const isTauriRuntime = vi.fn(() => true);

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listen(...args),
}));

vi.mock("../lib/tauri", () => ({
  isTauriRuntime: () => isTauriRuntime(),
  api: {
    translateSection: (sectionId: string) => translateSection(sectionId),
    cancelSectionTranslation: () => cancelSectionTranslation(),
  },
}));

const { useTranslationStore } = await import("./translation");

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.clearAllMocks();
  isTauriRuntime.mockReturnValue(true);
  listen.mockResolvedValue(() => undefined);
  useTranslationStore.setState({
    sectionState: {
      status: "idle",
      done: 0,
      total: 0,
      fallbackCount: 0,
      sentenceCount: 0,
      error: null,
    },
  });
});

describe("translation store", () => {
  it("applies progress for the section being translated", async () => {
    const result = deferred<Domain.TranslateSectionResult>();
    translateSection.mockReturnValue(result.promise);
    const progressHandlers: Array<
      (event: { payload: Domain.TranslationProgress }) => void
    > = [];
    listen.mockImplementation(
      async (
        _name: string,
        handler: (event: { payload: Domain.TranslationProgress }) => void,
      ) => {
        progressHandlers.push(handler);
        return () => undefined;
      },
    );

    const running = useTranslationStore.getState().translateSection("sec-1");
    await vi.waitFor(() => expect(listen).toHaveBeenCalled());
    progressHandlers[0]({
      payload: { sectionId: "sec-other", done: 99, total: 100 },
    });
    expect(useTranslationStore.getState().sectionState.done).toBe(0);

    progressHandlers[0]({
      payload: { sectionId: "sec-1", done: 40, total: 312 },
    });
    expect(useTranslationStore.getState().sectionState).toMatchObject({
      status: "running",
      done: 40,
      total: 312,
    });

    result.resolve({
      status: "complete",
      sourceLang: "en",
      targetLang: "es",
      fallbackCount: 9,
      sentenceCount: 312,
    });
    await running;

    expect(useTranslationStore.getState().sectionState).toEqual({
      status: "complete",
      done: 312,
      total: 312,
      fallbackCount: 9,
      sentenceCount: 312,
      error: null,
    });
  });

  it("asks Rust to cancel the chapter in flight", async () => {
    const result = deferred<Domain.TranslateSectionResult>();
    translateSection.mockReturnValue(result.promise);
    const running = useTranslationStore.getState().translateSection("sec-1");
    await vi.waitFor(() => expect(translateSection).toHaveBeenCalled());

    useTranslationStore.getState().cancel();
    expect(cancelSectionTranslation).toHaveBeenCalledTimes(1);
    expect(useTranslationStore.getState().sectionState.status).toBe("running");

    result.resolve({
      status: "cancelled",
      sourceLang: "en",
      targetLang: "es",
      done: 40,
      total: 312,
      fallbackCount: 272,
      sentenceCount: 312,
    });
    await running;
    expect(useTranslationStore.getState().sectionState.status).toBe("idle");
  });

  it("turns a missing model into an actionable Reader error", async () => {
    translateSection.mockResolvedValue({
      status: "needsDownload",
      sourceLang: "en",
      targetLang: "es",
      modelStatus: {
        downloaded: false,
        downloadedBytes: 0,
        totalBytes: 250_000_000,
        verified: true,
      },
    } satisfies Domain.TranslateSectionResult);

    await useTranslationStore.getState().translateSection("sec-1");

    expect(useTranslationStore.getState().sectionState).toMatchObject({
      status: "failed",
      error: expect.stringMatching(/download.*Settings/i),
    });
  });
});
