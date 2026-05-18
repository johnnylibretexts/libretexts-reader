import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Filter, Loader2 } from "lucide-react";
import { api } from "../../lib/tauri";
import type * as Domain from "../../types/domain";
import { VoiceCard } from "./VoiceCard";

interface VoiceProgress {
  voiceId: string;
  downloaded: number;
  total: number;
}

export function VoiceGallery() {
  const [voices, setVoices] = useState<Domain.Voice[]>([]);
  const [language, setLanguage] = useState("all");
  const [gender, setGender] = useState("all");
  const [loading, setLoading] = useState(true);
  const [busyVoiceId, setBusyVoiceId] = useState<string | null>(null);
  const [previewingVoiceId, setPreviewingVoiceId] = useState<string | null>(
    null,
  );
  const [progress, setProgress] = useState<Record<string, VoiceProgress>>({});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void refreshVoices();
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<VoiceProgress>("voice-download-progress", (event) => {
      setProgress((current) => ({
        ...current,
        [event.payload.voiceId]: event.payload,
      }));
    })
      .then((dispose) => {
        unlisten = dispose;
      })
      .catch(() => undefined);

    return () => {
      unlisten?.();
    };
  }, []);

  const languages = useMemo(
    () => uniqueOptions(voices.map((voice) => voice.language)),
    [voices],
  );
  const genders = useMemo(
    () => uniqueOptions(voices.map((voice) => voice.gender)),
    [voices],
  );
  const filteredVoices = useMemo(
    () =>
      voices.filter((voice) => {
        const matchesLanguage = language === "all" || voice.language === language;
        const matchesGender = gender === "all" || voice.gender === gender;
        return matchesLanguage && matchesGender;
      }),
    [gender, language, voices],
  );

  async function refreshVoices() {
    setLoading(true);
    setError(null);
    try {
      setVoices(await api.listVoices());
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setLoading(false);
    }
  }

  async function downloadVoice(voice: Domain.Voice) {
    setBusyVoiceId(voice.id);
    setError(null);
    try {
      await api.downloadVoice(voice.id);
      await refreshVoices();
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusyVoiceId(null);
      setProgress((current) => {
        const next = { ...current };
        delete next[voice.id];
        return next;
      });
    }
  }

  async function deleteVoice(voice: Domain.Voice) {
    setBusyVoiceId(voice.id);
    setError(null);
    try {
      await api.deleteVoice(voice.id);
      await refreshVoices();
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusyVoiceId(null);
    }
  }

  async function previewVoice(voice: Domain.Voice) {
    setPreviewingVoiceId(voice.id);
    setError(null);
    try {
      await playPreviewTone();
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setPreviewingVoiceId(null);
    }
  }

  return (
    <section className="flex flex-col gap-4">
      <div className="grid gap-3 rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900 sm:grid-cols-2">
        <label className="flex flex-col gap-2 text-sm font-medium">
          Language
          <span className="relative">
            <Filter
              className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-neutral-400"
              aria-hidden="true"
            />
            <select
              className="h-10 w-full rounded-md border border-neutral-200 bg-white pl-9 pr-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
              onChange={(event) => setLanguage(event.target.value)}
              value={language}
            >
              <option value="all">All</option>
              {languages.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          </span>
        </label>

        <label className="flex flex-col gap-2 text-sm font-medium">
          Gender
          <select
            className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal capitalize outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
            onChange={(event) => setGender(event.target.value)}
            value={gender}
          >
            <option value="all">All</option>
            {genders.map((option) => (
              <option className="capitalize" key={option} value={option}>
                {option}
              </option>
            ))}
          </select>
        </label>
      </div>

      {error ? (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {error}
        </p>
      ) : null}

      {loading ? (
        <p className="inline-flex items-center gap-2 text-sm text-neutral-500 dark:text-neutral-400">
          <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          Loading...
        </p>
      ) : null}

      {!loading && filteredVoices.length === 0 ? (
        <p className="rounded-md border border-dashed border-neutral-300 px-4 py-8 text-center text-sm text-neutral-500 dark:border-neutral-700 dark:text-neutral-400">
          No voices match the selected filters.
        </p>
      ) : null}

      {filteredVoices.length > 0 ? (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {filteredVoices.map((voice) => (
            <VoiceCard
              disabled={Boolean(busyVoiceId)}
              key={voice.id}
              onDelete={(voice) => void deleteVoice(voice)}
              onDownload={(voice) => void downloadVoice(voice)}
              onPreview={(voice) => void previewVoice(voice)}
              previewing={previewingVoiceId === voice.id}
              progress={progress[voice.id] ?? null}
              voice={voice}
            />
          ))}
        </div>
      ) : null}
    </section>
  );
}

async function playPreviewTone() {
  const AudioContextClass = window.AudioContext;
  const audioContext = new AudioContextClass();
  const oscillator = audioContext.createOscillator();
  const gain = audioContext.createGain();

  oscillator.frequency.value = 440;
  gain.gain.value = 0.03;
  oscillator.connect(gain);
  gain.connect(audioContext.destination);
  oscillator.start();

  await new Promise((resolve) => window.setTimeout(resolve, 240));
  oscillator.stop();
  await audioContext.close();
}

function uniqueOptions(values: string[]) {
  return [...new Set(values.filter(Boolean))].sort((left, right) =>
    left.localeCompare(right),
  );
}
