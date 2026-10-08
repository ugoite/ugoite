import { describe, expect, it } from "vitest";
import { extractPwaHeadTags } from "./static-index.ts";

describe("static SPA PWA head tags", () => {
  it("preserves manifest and worker registration tags emitted by VitePWA", () => {
    const html = `<!doctype html>
<html><head>
<link rel="manifest" href="/app.webmanifest" crossorigin="use-credentials">
<script id="vite-plugin-pwa:register-sw" src="/registerSW.js"></script>
</head><body></body></html>`;

    expect(extractPwaHeadTags(html)).toBe(
      '<link rel="manifest" href="/app.webmanifest" crossorigin="use-credentials">\n\t\t<script id="vite-plugin-pwa:register-sw" src="/registerSW.js"></script>',
    );
  });

  it("fails the static build when VitePWA registration was not emitted", () => {
    expect(() => extractPwaHeadTags("<html><head></head></html>")).toThrow(
      "The generated index must include the Vite PWA manifest and service worker registration tags",
    );
  });
});
