import { describe, expect, it } from "vitest";
import { asAppError, displayError } from "./errors";

const backendError = { kind: "drm_protected", message: "DRM-protected content cannot be imported", retryable: false };

describe("asAppError", () => {
  it("narrows a structured backend rejection", () => {
    const error = asAppError(backendError);

    expect(error).not.toBeNull();
    expect(error?.kind).toBe("drm_protected");
    expect(error?.retryable).toBe(false);
  });

  it("accepts a kind the union does not know yet", () => {
    // Rust may gain a variant before domain.ts mirrors it. The message should
    // still reach the user; check-error-kinds.sh is what catches the drift.
    const error = asAppError({ kind: "brand_new", message: "something broke", retryable: true });

    expect(error?.message).toBe("something broke");
  });

  it.each([
    ["a plain Error", new Error("kokoro failed to load")],
    ["a string", "DESKTOP_RUNTIME_ERROR"],
    ["null", null],
    ["a partial shape", { kind: "io", message: "no retryable field" }],
    ["a wrongly typed field", { kind: "io", message: "x", retryable: "yes" }],
  ])("returns null for %s", (_label, input) => {
    expect(asAppError(input)).toBeNull();
  });
});

describe("displayError", () => {
  it("shows the backend message without repeating the kind", () => {
    expect(displayError(backendError)).toBe("DRM-protected content cannot be imported");
  });

  it("falls back to the message of a non-backend Error", () => {
    expect(displayError(new Error("kokoro failed to load"))).toBe("kokoro failed to load");
  });

  it("stringifies anything else", () => {
    expect(displayError("DESKTOP_RUNTIME_ERROR")).toBe("DESKTOP_RUNTIME_ERROR");
  });
});
