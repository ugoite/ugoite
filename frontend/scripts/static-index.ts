const buildBase = "/_build/";

export const buildPwaHeadTags = (): string =>
  [
    `<link rel="manifest" href="${buildBase}manifest.webmanifest">`,
    `<script id="vite-plugin-pwa:register-sw" src="${buildBase}registerSW.js"></script>`,
  ].join("\n\t\t");
