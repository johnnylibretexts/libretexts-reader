import { create } from "zustand";
import type {
  SupertonicLanguage,
  SupertonicVoiceStyle,
} from "../lib/supertonic";

/**
 * The settings snapshot the export drafts were seeded from: the reader's
 * stored rows, or the built-ins the store reports while a load has failed.
 * `null` before either exists.
 */
export type SeedSignal = "defaults" | "stored" | null;

/**
 * The chapter-export panel's own Voice and Language, for as long as the app is
 * open.
 *
 * Deliberately a store and not `useState` in the panel. `AppShell`
 * switch-renders routes, so a trip to the Library unmounts the Reader outright
 * and took the pick with it -- the reader exported chapter 1 in Female 3, came
 * back, and chapter 2 was set to the app's reading voice with the dropdown
 * showing it and nothing saying their pick had gone.
 *
 * Equally deliberately *not* the `supertonic_voice_style` / `supertonic_language`
 * settings rows. The panel used to write those on every Preview and Generate;
 * once playback started reading them, that write switched the narration of the
 * book open in the same view, mid-chapter, and left Settings displaying a voice
 * the reader never chose there. #60 removed it, and this must not reintroduce
 * it: Settings owns the reading voice, this owns a voice for one file.
 *
 * Session-scoped on purpose. A relaunch re-seeds from the reader's Settings
 * voice, which is the right starting point for a fresh session; persisting it
 * would make the export voice stored app state again, which is the thing #60
 * was about.
 */
export interface ChapterExportState {
  voiceStyle: SupertonicVoiceStyle | null;
  language: SupertonicLanguage | null;
  /** Which settings snapshot {@link voiceStyle} and {@link language} came from. */
  seededFrom: SeedSignal;
  /**
   * Whether the reader picked this draft themselves.
   *
   * One flag per draft, never a shared one: they are independent picks, and a
   * single flag would make re-seeding all-or-nothing -- touching Voice would
   * freeze Language on whatever it happened to hold.
   *
   * These live here rather than in the panel for the same reason the drafts do.
   * As `useRef`s they reset on unmount, so the seeding effect ran again on the
   * way back into the Reader and overwrote the remembered pick. Moving the
   * drafts without moving these would have fixed nothing.
   */
  voiceChosen: boolean;
  languageChosen: boolean;
}

export interface ChapterExportStore extends ChapterExportState {
  /** Record a voice the reader picked, so no later seed may replace it. */
  chooseVoiceStyle: (voiceStyle: SupertonicVoiceStyle) => void;
  /** Record a language the reader picked, so no later seed may replace it. */
  chooseLanguage: (language: SupertonicLanguage) => void;
  /**
   * Seed the drafts the reader has not picked from the app's settings.
   *
   * Takes both values and the snapshot they came from, and leaves any draft
   * the reader chose alone. Recording `seededFrom` here is what lets a retry
   * that finally brings the real rows in re-seed once, and only once.
   */
  seed: (
    from: Exclude<SeedSignal, null>,
    settings: {
      supertonicVoiceStyle: SupertonicVoiceStyle;
      supertonicLanguage: SupertonicLanguage;
    },
  ) => void;
  /** Back to a fresh session. Exists for tests, which share one module store. */
  reset: () => void;
}

const EMPTY: ChapterExportState = {
  voiceStyle: null,
  language: null,
  seededFrom: null,
  voiceChosen: false,
  languageChosen: false,
};

export const useChapterExportStore = create<ChapterExportStore>((set) => ({
  ...EMPTY,

  chooseVoiceStyle: (voiceStyle) => set({ voiceStyle, voiceChosen: true }),

  chooseLanguage: (language) => set({ language, languageChosen: true }),

  seed: (from, settings) =>
    set((state) => ({
      voiceStyle: state.voiceChosen
        ? state.voiceStyle
        : settings.supertonicVoiceStyle,
      language: state.languageChosen
        ? state.language
        : settings.supertonicLanguage,
      seededFrom: from,
    })),

  reset: () => set({ ...EMPTY }),
}));
