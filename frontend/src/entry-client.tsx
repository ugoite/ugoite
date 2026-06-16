// @refresh reload
import { mount, StartClient } from "@solidjs/start/client";
import { render } from "solid-js/web";

declare const __UGOITE_STATIC_SPA__: boolean;

const app = document.getElementById("app");
if (!app) throw new Error("App element not found");
if (__UGOITE_STATIC_SPA__) {
  render(() => <StartClient />, app);
} else {
  mount(() => <StartClient />, app);
}

/**
 * Fundamental solution for Vinxi/SolidStart entry-client default export warning
 */
export default {};
