import { describe, expect, it } from "vitest";
import { BRAND_ICON_LINKS, brandIconLinksHtml } from "./brand-icon-links";
import { getBrandIconPrecacheEntries } from "./brand-icon-precache";

describe("brand icon declarations", () => {
  it("declares the same favicon and touch icon set for server and static HTML", () => {
    expect(brandIconLinksHtml()).toBe([
      '<link rel="icon" sizes="any" href="/favicon.ico">',
      '<link rel="icon" type="image/svg+xml" href="/brand/ugoite-icon-square.svg">',
      '<link rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png">',
    ].join("\n"));
  });

  it("keeps the original transparent mark separate from app icons", () => {
    expect(BRAND_ICON_LINKS.map(({ href }) => href)).toEqual([
      "/favicon.ico",
      "/brand/ugoite-icon-square.svg",
      "/apple-touch-icon.png",
    ]);
    expect(BRAND_ICON_LINKS).not.toContainEqual(
      expect.objectContaining({ href: "/brand/ugoite-mark.svg" }),
    );
  });

  it("precaches each generated public icon using its content hash", () => {
    expect(getBrandIconPrecacheEntries([
      { path: "frontend/public/favicon.ico", sha256: "ico-hash" },
      {
        path: "frontend/public/icons/ugoite-192.png",
        sha256: "192-hash",
      },
      { path: "docs/brand/assets/social.png", sha256: "social-hash" },
    ])).toEqual([
      { url: "/favicon.ico", revision: "ico-hash" },
      { url: "/icons/ugoite-192.png", revision: "192-hash" },
    ]);
  });
});
