import { createRoot, createSignal } from "solid-js";
import { isServer } from "solid-js/web";
import {
  readLocalPreferences,
  writeLocalPreferences,
} from "./preferences-local";

export type ColorMode = "light" | "dark";

const readStoredMode = (): ColorMode | null => {
  const value = readLocalPreferences().color_mode;
  return value === "light" || value === "dark" ? value : null;
};

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

const modeStore = createRoot(() => {
  const [mode, setMode] = createSignal<ColorMode>(
    readStoredMode() ?? resolveSystemMode(),
  );

  return { mode, setMode };
});

export const colorMode = modeStore.mode;
export const setColorMode = modeStore.setMode;

export const initializeColorMode = () => applyColorMode(colorMode());

export const applyColorModePreference = (mode: ColorMode) => {
  setColorMode(mode);
  applyColorMode(mode);
  writeLocalPreferences({ color_mode: mode });
};
