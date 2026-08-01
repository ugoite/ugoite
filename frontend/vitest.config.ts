import { defineConfig } from "vitest/config";
import solidPlugin from "vite-plugin-solid";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

const frontendRoot = fileURLToPath(new URL(".", import.meta.url));
const frontendTestOrigin = process.env.FRONTEND_TEST_ORIGIN ??
  "http://localhost:3000";
const materializedSolidJs = fileURLToPath(
  new URL("./node_modules/solid-js/dist/dev.js", import.meta.url),
);
const materializedSolidJsWeb = fileURLToPath(
  new URL("./node_modules/solid-js/web/dist/dev.js", import.meta.url),
);
const materializedSolidJsStore = fileURLToPath(
  new URL("./node_modules/solid-js/store/dist/dev.js", import.meta.url),
);

// The production build materializes Solid under frontend/node_modules. Keep
// npm dependencies on that same runtime when tests run after the build.
const solidAliases = existsSync(materializedSolidJs)
  ? [
    { find: "solid-js/web", replacement: materializedSolidJsWeb },
    { find: "solid-js/store", replacement: materializedSolidJsStore },
    { find: "solid-js", replacement: materializedSolidJs },
  ]
  : [];

export default defineConfig({
  root: frontendRoot,
  plugins: [solidPlugin() as never],
  define: {
    "process.env.NODE_ENV": JSON.stringify("test"),
    "process.env.FRONTEND_TEST_ORIGIN": JSON.stringify(frontendTestOrigin),
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    env: {
      FRONTEND_TEST_ORIGIN: frontendTestOrigin,
    },
    include: ["src/**/*.{test,spec}.{js,ts,tsx}"],
    // The UI owns shared browser-global state (locale, document attributes,
    // and localStorage). Running files concurrently makes that state leak
    // across otherwise isolated tests.
    fileParallelism: false,
    testTimeout: 10000,
    server: {
      deps: {
        inline: ["@solidjs/router", "@solidjs/testing-library"],
      },
    },
    coverage: {
      provider: "v8",
      reporter: ["text", "json", "html"],
      exclude: [
        "src/test/**",
        "src/**/*.test.*",
        "src/**/*.spec.*",
        // Framework boilerplate - cannot be unit-tested in isolation
        "src/entry-client.tsx",
        "src/entry-server.tsx",
        "src/global.d.ts",
        "src/error-handler.ts",
        "src/app.tsx",
        // Route components - SolidStart SSR framework glue
        "src/routes/**",
        // Type-only and barrel re-export files
        "src/lib/types.ts",
        "src/lib/index.ts",
        "src/components/index.ts",
      ],
      thresholds: {
        lines: 100,
        functions: 100,
        branches: 100,
        statements: 100,
      },
    },
  },
  resolve: {
    conditions: ["development", "browser"],
    alias: [{ find: "~", replacement: "/src" }, ...solidAliases],
    dedupe: [
      "@solidjs/router",
      "@solidjs/start",
      "@codemirror/autocomplete",
      "@codemirror/lang-sql",
      "@codemirror/lint",
      "@codemirror/state",
      "@codemirror/view",
      "solid-js",
      "solid-js/web",
    ],
  },
});
