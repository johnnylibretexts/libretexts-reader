import { listen } from "@tauri-apps/api/event";
import { create, type StoreApi, type UseBoundStore } from "zustand";
import { displayError } from "../lib/errors";
import { api, isTauriRuntime } from "../lib/tauri";
import type * as Domain from "../types/domain";

export type TranslationSectionState = {
  status: "idle" | "running" | "complete" | "failed";
  done: number;
  total: number;
  /** Sentences reading in the original language after QA. */
  fallbackCount: number;
  sentenceCount: number;
  error: string | null;
};

export type TranslationStore = {
  sectionState: TranslationSectionState;
  translateSection: (sectionId: string) => Promise<void>;
  cancel: () => void;
};

const IDLE_SECTION: TranslationSectionState = {
  status: "idle",
  done: 0,
  total: 0,
  fallbackCount: 0,
  sentenceCount: 0,
  error: null,
};

let runSequence = 0;
let activeSectionId: string | null = null;
let activePromise: Promise<void> | null = null;

export const useTranslationStore: UseBoundStore<StoreApi<TranslationStore>> =
  create<TranslationStore>((set, get) => ({
    sectionState: IDLE_SECTION,

    translateSection: async (sectionId) => {
      if (
        activePromise &&
        activeSectionId === sectionId &&
        get().sectionState.status === "running"
      ) {
        return activePromise;
      }

      const sequence = ++runSequence;
      activeSectionId = sectionId;
      set({
        sectionState: {
          ...IDLE_SECTION,
          status: "running",
        },
      });

      const run = (async () => {
        let unlisten: (() => void) | null = null;
        try {
          if (isTauriRuntime()) {
            unlisten = await listen<Domain.TranslationProgress>(
              "translation-progress",
              (event) => {
                if (
                  sequence !== runSequence ||
                  event.payload.sectionId !== sectionId
                ) {
                  return;
                }
                set((state) => ({
                  sectionState: {
                    ...state.sectionState,
                    status: "running",
                    done: event.payload.done,
                    total: event.payload.total,
                    sentenceCount: event.payload.total,
                    error: null,
                  },
                }));
              },
            );
          }

          const result = await api.translateSection(sectionId);
          if (sequence !== runSequence) {
            return;
          }
          set({ sectionState: stateFromResult(result) });
        } catch (error) {
          if (sequence === runSequence) {
            set({
              sectionState: {
                ...get().sectionState,
                status: "failed",
                error: displayError(error),
              },
            });
          }
        } finally {
          unlisten?.();
          if (sequence === runSequence) {
            activePromise = null;
            activeSectionId = null;
          }
        }
      })();

      activePromise = run;
      return run;
    },

    cancel: () => {
      // Keep the chapter in `running` until the cooperative backend command
      // returns `cancelled`. That keeps Play and section navigation disabled
      // while the current batch winds down, so a second translation cannot
      // clear Rust's shared cancel flag and overlap the first one.
      void api.cancelSectionTranslation().catch((error) => {
        set({
          sectionState: {
            ...IDLE_SECTION,
            status: "failed",
            error: `The translation may still be running because it could not be told to stop. (${displayError(error)})`,
          },
        });
      });
    },
  }));

function stateFromResult(
  result: Domain.TranslateSectionResult,
): TranslationSectionState {
  switch (result.status) {
    case "original":
      return {
        status: "complete",
        done: result.sentenceCount,
        total: result.sentenceCount,
        fallbackCount: 0,
        sentenceCount: result.sentenceCount,
        error: null,
      };
    case "needsDownload":
      return {
        ...IDLE_SECTION,
        status: "failed",
        error: `Download the ${result.sourceLang.toUpperCase()} → ${result.targetLang.toUpperCase()} translation models in Settings before playing this chapter.`,
      };
    case "complete":
      return {
        status: "complete",
        done: result.sentenceCount,
        total: result.sentenceCount,
        fallbackCount: result.fallbackCount,
        sentenceCount: result.sentenceCount,
        error: null,
      };
    case "cancelled":
      return {
        status: "idle",
        done: result.done,
        total: result.total,
        fallbackCount: result.fallbackCount,
        sentenceCount: result.sentenceCount,
        error: null,
      };
  }
}
