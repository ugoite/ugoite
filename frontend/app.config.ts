import type { ProxyOptions } from "vite";
import { readFileSync } from "node:fs";
import { getBrandIconPrecacheEntries } from "./src/lib/brand-icon-precache.ts";

type ProcessLike = {
  env?: Record<string, string | undefined>;
};

const processLike = globalThis.process as ProcessLike | undefined;
const currentEnv = processLike?.env;
if (!currentEnv || typeof currentEnv !== "object") {
  /* v8 ignore start */
  if (processLike) {
    Object.defineProperty(processLike, "env", {
      configurable: true,
      writable: true,
      value: {},
    });
  } else {
    Object.defineProperty(globalThis, "process", {
      configurable: true,
      writable: true,
      value: {
        env: {},
        cwd: () => new URL(".", import.meta.url).pathname,
      },
    });
  }
  /* v8 ignore stop */
}

const denoEnv = (globalThis as typeof globalThis & {
  Deno?: { env: { toObject(): Record<string, string> } };
}).Deno?.env.toObject() ?? {};
/* v8 ignore next */
const env = {
  ...denoEnv,
  ...((globalThis.process as ProcessLike | undefined)?.env ?? {}),
};

const [{ defineConfig }, { default: tailwindcss }, { VitePWA }] = await Promise
  .all([
    import("@solidjs/start/config"),
    import("@tailwindcss/vite"),
    import("vite-plugin-pwa"),
  ]);

const backendUrl = env.BACKEND_URL;
const useViteProxy = env.VITE_API_PROXY === "true";
const staticSpa = env.UGOITE_STATIC_SPA === "true";

const sharedDir = new URL("../shared", import.meta.url).pathname;
const brandIconManifest = JSON.parse(
  readFileSync(
    new URL("../docs/brand/assets/manifest.json", import.meta.url),
    "utf8",
  ),
) as {
  outputs: Array<{ path: string; sha256: string }>;
};
const brandIconPrecacheEntries = getBrandIconPrecacheEntries(
  brandIconManifest.outputs,
);

const proxyRule: Record<string, ProxyOptions> = {};

if (backendUrl && useViteProxy) {
  proxyRule["/api"] = {
    target: backendUrl,
    changeOrigin: true,
    secure: false,
    rewrite: (path: string) => path.replace(/^\/api/, ""),
  };
} else if (env.NODE_ENV === "development") {
  throw new Error(
    "BACKEND_URL must be set for frontend-only development. Use `mise run dev` from the repository root for the canonical auth-aware workflow, or set BACKEND_URL=http://localhost:8000 only when you intentionally run `mise run //frontend:dev` against an already reachable backend.",
  );
}

export default defineConfig({
  ssr: !staticSpa,
  server: {
    errorHandler: "~/error-handler",
  },
  vite: {
    define: {
      __UGOITE_STATIC_SPA__: JSON.stringify(staticSpa),
    },
    plugins: [
      tailwindcss(),
      VitePWA({
        registerType: "autoUpdate",
        injectRegister: "auto",
        includeAssets: [
          "favicon.ico",
          "brand/ugoite-icon-square.svg",
          "apple-touch-icon.png",
          "icons/ugoite-192.png",
          "icons/ugoite-512.png",
        ],
        manifest: {
          name: "Ugoite",
          short_name: "Ugoite",
          description: "Local-first, AI-native knowledge space",
          theme_color: "#111827",
          background_color: "#111827",
          display: "standalone",
          start_url: "/",
          scope: "/",
          icons: [
            {
              src: "/icons/ugoite-192.png",
              sizes: "192x192",
              type: "image/png",
              purpose: "any",
            },
            {
              src: "/icons/ugoite-512.png",
              sizes: "512x512",
              type: "image/png",
              purpose: "any",
            },
          ],
        },
        workbox: {
          globPatterns: ["**/*.{js,css,html,ico,png,svg}"],
          additionalManifestEntries: brandIconPrecacheEntries,
        },
      }),
    ],
    server: {
      proxy: proxyRule,
      fs: {
        allow: [sharedDir],
      },
    },
    resolve: {
      conditions: ["development", "browser", "solid"],
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
    ssr: {
      resolve: {
        conditions: ["development", "node", "solid"],
      },
    },
  },
});
