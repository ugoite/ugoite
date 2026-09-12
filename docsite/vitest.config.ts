import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    globals: true,
    include: ["src/**/*.{test,spec}.{js,mjs,ts,tsx}"],
    testTimeout: 10000,
    coverage: {
      // Coverage stays available as developer convenience (`deno task coverage`).
      // It is not a required merge gate: documentation navigation changes must
      // not fail the repository quality lane on uncovered docsite shell lines.
      provider: "v8",
      reporter: ["text", "json", "html"],
      include: ["src/**/*.{js,mjs,ts,tsx}"],
      exclude: [
        "src/**/*.test.*",
        "src/**/*.spec.*",
        "src/env.d.ts",
        // Astro's virtual-module wiring is outside this unit-coverage scope.
        "src/content.config.ts",
      ],
    },
  },
});
