export const extractPwaHeadTags = (html: string): string => {
  const manifestTag = html.match(
    /<link\b(?=[^>]*\srel=["']manifest["'])[^>]*>/i,
  )?.[0];
  const registrationTag = html.match(
    /<script\b(?=[^>]*\sid=["']vite-plugin-pwa:register-sw["'])[^>]*>\s*<\/script>/i,
  )?.[0];

  if (!manifestTag || !registrationTag) {
    throw new Error(
      "The generated index must include the Vite PWA manifest and service worker registration tags",
    );
  }

  return `${manifestTag}\n\t\t${registrationTag}`;
};
