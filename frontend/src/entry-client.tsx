// @refresh reload
import { mount, StartClient } from "@solidjs/start/client";
import { installVitePreloadRecovery } from "~/lib/vite-preload-recovery";

installVitePreloadRecovery();

const app = document.getElementById("app");
if (!app) throw new Error("App element not found");
mount(() => <StartClient />, app);
