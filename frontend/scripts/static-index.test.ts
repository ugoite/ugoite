import { describe, expect, it } from "vitest";
import { buildPwaHeadTags } from "./static-index.ts";

describe("static SPA PWA head tags", () => {
  it("links the generated manifest and registration script from the client build", () => {
    expect(buildPwaHeadTags()).toBe(
      '<link rel="manifest" href="/_build/manifest.webmanifest">\n\t\t<script id="vite-plugin-pwa:register-sw" src="/_build/registerSW.js"></script>',
    );
  });
});
