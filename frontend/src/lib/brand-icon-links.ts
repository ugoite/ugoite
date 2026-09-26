export const BRAND_ICON_LINKS = [
  { rel: "icon", href: "/favicon.ico", sizes: "any" },
  {
    rel: "icon",
    type: "image/svg+xml",
    href: "/brand/ugoite-icon-square.svg",
  },
  {
    rel: "apple-touch-icon",
    sizes: "180x180",
    href: "/apple-touch-icon.png",
  },
] as const;

export function brandIconLinksHtml(): string {
  return BRAND_ICON_LINKS.map((link) =>
    `<link rel="${link.rel}"${"type" in link ? ` type="${link.type}"` : ""}${
      "sizes" in link ? ` sizes="${link.sizes}"` : ""
    } href="${link.href}">`
  ).join("\n");
}
