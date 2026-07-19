import { isServer } from "solid-js/web";

export type ColorMode = "light" | "dark";

const resolveSystemMode = (): ColorMode => {
  if (isServer || typeof window === "undefined") return "light";
  if (typeof window.matchMedia !== "function") return "light";
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
};

const applyColorMode = (mode: ColorMode) => {
  if (isServer || typeof document === "undefined") return;
  document.documentElement.dataset.colorMode = mode;
};

let systemModeQuery: MediaQueryList | undefined;
let systemModeListenerAttached = false;

export const colorMode = resolveSystemMode;
export const initializeColorMode = () => {
  if (isServer || typeof window === "undefined") return;
  if (typeof window.matchMedia !== "function") return;

  systemModeQuery ??= window.matchMedia("(prefers-color-scheme: dark)");
  applyColorMode(systemModeQuery.matches ? "dark" : "light");
  if (systemModeListenerAttached) return;

  systemModeQuery.addEventListener("change", (event) => {
    applyColorMode(event.matches ? "dark" : "light");
  });
  systemModeListenerAttached = true;
};
