/* @vitest-environment node */
import { afterEach, describe, expect, it, vi } from "vitest";

const loadConfig = async () => import("../app.config.ts");

afterEach(() => {
	vi.unstubAllEnvs();
	vi.resetModules();
});

describe("frontend app config", () => {
	it("REQ-OPS-010: missing BACKEND_URL points contributors to the canonical root dev workflow", async () => {
		vi.stubEnv("NODE_ENV", "development");
		vi.stubEnv("VITE_API_PROXY", "false");
		vi.stubEnv("BACKEND_URL", "");

		let message = "";
		try {
			await loadConfig();
		} catch (error) {
			message = error instanceof Error ? error.message : String(error);
		}

		expect(message).toMatch(/Use `mise run dev` from the repository root/);
		expect(message).toMatch(/`mise run \/\/frontend:dev` against an already reachable backend/);
	});
});
