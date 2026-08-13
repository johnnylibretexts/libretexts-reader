import { defineConfig } from "vitest/config";

// Kept separate from vite.config.ts so the app build (and `tauri build`) never
// has to load vitest.
export default defineConfig({
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
  },
});
