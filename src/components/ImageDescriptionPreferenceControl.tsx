import { useState } from "react";
import { displayError } from "../lib/errors";
import { useSettingsStore } from "../stores/settings";

export function ImageDescriptionPreferenceControl({
  compact = false,
}: {
  compact?: boolean;
}) {
  const enabled = useSettingsStore(
    (state) => state.readImageDescriptionsAutomatically,
  );
  const setEnabled = useSettingsStore(
    (state) => state.setReadImageDescriptionsAutomatically,
  );
  const hydrateFailed = useSettingsStore((state) => state.hydrateFailed);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function change(next: boolean) {
    setSaving(true);
    setError(null);
    try {
      await setEnabled(next);
    } catch (cause) {
      setError(displayError(cause));
    } finally {
      setSaving(false);
    }
  }

  if (compact) {
    return (
      <div>
        <label className="inline-flex h-10 items-center gap-2 rounded-md border border-neutral-200 px-3 text-sm font-medium text-neutral-700 dark:border-neutral-800 dark:text-neutral-200">
          <input
            aria-describedby={error ? "reader-image-description-error" : undefined}
            checked={enabled}
            className="size-4 accent-brand-700"
            disabled={saving || hydrateFailed}
            onChange={(event) => void change(event.target.checked)}
            type="checkbox"
          />
          Describe images
        </label>
        {error ? (
          <span
            className="mt-1 block max-w-64 text-xs text-red-700 dark:text-red-300"
            id="reader-image-description-error"
            role="alert"
          >
            {error}
          </span>
        ) : null}
      </div>
    );
  }

  return (
    <div>
      <label className="flex items-start gap-3">
        <input
          aria-describedby="image-description-help"
          checked={enabled}
          className="mt-0.5 size-4 accent-brand-700"
          disabled={saving || hydrateFailed}
          onChange={(event) => void change(event.target.checked)}
          type="checkbox"
        />
        <span>
          <span className="block text-sm font-medium">
            Read image descriptions automatically
          </span>
          <span
            className="mt-1 block text-xs text-neutral-500 dark:text-neutral-400"
            id="image-description-help"
          >
            Reads publisher-provided alt text after the paragraph it belongs
            to. Uses the current voice and Listen in language. Enabled by
            default for accessibility.
          </span>
        </span>
      </label>
      {error ? (
        <p className="mt-2 text-sm text-red-700 dark:text-red-300" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}
