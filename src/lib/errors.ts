import type { AppError } from "../types/domain";

/**
 * Narrow an unknown rejection to the structured error the Rust backend sends
 * across the invoke boundary.
 *
 * Returns null for anything else — webview-side failures, the
 * desktop-runtime guard in `tauri.ts`, and plain JS exceptions all reach
 * callers unchanged. Never throws.
 *
 * An unrecognised `kind` is deliberately still accepted: if the Rust side
 * gains a variant before this union does, the message should still reach the
 * user. `scripts/ci/check-error-kinds.sh` is what keeps the two in step.
 */
export function asAppError(error: unknown): AppError | null {
  if (typeof error !== "object" || error === null) {
    return null;
  }

  const candidate = error as Record<string, unknown>;
  if (
    typeof candidate.kind !== "string" ||
    typeof candidate.message !== "string" ||
    typeof candidate.retryable !== "boolean"
  ) {
    return null;
  }

  return candidate as unknown as AppError;
}

/** The message to show a user, whatever the rejection turned out to be. */
export function displayError(error: unknown): string {
  const appError = asAppError(error);
  if (appError) {
    return appError.message;
  }

  return error instanceof Error ? error.message : String(error);
}
