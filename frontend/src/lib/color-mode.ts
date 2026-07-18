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

export const colorMode = resolveSystemMode;
export const initializeColorMode = () => applyColorMode(resolveSystemMode());
