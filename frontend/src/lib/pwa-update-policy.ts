export const pwaUpdatePolicy = {
  registerType: "prompt",
  workbox: {
    skipWaiting: false,
    clientsClaim: true,
    navigateFallback: null,
  },
} as const;
