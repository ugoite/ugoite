/* @vitest-environment node */
import { afterEach, describe, expect, it, vi } from "vitest";

type LoadedConfig = Awaited<typeof import("../app.config.ts")>;
type ViteUserConfig = {
	server?: {
		proxy?: Record<
			string,
			{
				target?: string;
				changeOrigin?: boolean;
				secure?: boolean;
				rewrite?: (path: string) => string;
			}
		>;
	};
};
type ConfigPlugin = {
	name?: string;
	config?: () => Promise<ViteUserConfig> | ViteUserConfig;
};

const loadConfig = async (): Promise<LoadedConfig["default"]> =>
	(await import("../app.config.ts")).default;

const loadClientUserConfig = async (): Promise<ViteUserConfig> => {
	const app = await loadConfig();
	const plugins = (await app.getRouter("client").plugins()) as ConfigPlugin[];
	const userPlugin = plugins.find((plugin) => plugin.name === "vinxi:config:user");
	if (!userPlugin?.config) {
		throw new Error("Expected vinxi user config plugin for the client router");
	}

	return await userPlugin.config();
};

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

	it("REQ-OPS-010: backend proxy settings are wired when frontend-only dev mode targets a backend", async () => {
		vi.stubEnv("NODE_ENV", "development");
		vi.stubEnv("VITE_API_PROXY", "true");
		vi.stubEnv("BACKEND_URL", "http://localhost:8000");

		const config = await loadClientUserConfig();
		const apiProxy = config.server?.proxy?.["/api"];
		if (!apiProxy) {
			throw new Error("Expected /api proxy rule to be configured");
		}

		expect(apiProxy.target).toBe("http://localhost:8000");
		expect(apiProxy.changeOrigin).toBe(true);
		expect(apiProxy.secure).toBe(false);
		expect(apiProxy.rewrite?.("/api/spaces")).toBe("/spaces");
	});

	it("REQ-OPS-010: non-development imports keep an empty proxy map when no backend URL is configured", async () => {
		vi.stubEnv("NODE_ENV", "production");
		vi.stubEnv("VITE_API_PROXY", "false");
		vi.stubEnv("BACKEND_URL", "");

		const config = await loadClientUserConfig();
		expect(Object.keys(config.server?.proxy ?? {})).toHaveLength(0);
	});

	it("REQ-OPS-010: config falls back to an empty env object when process.env is unavailable", async () => {
		const originalEnv = process.env;
		const originalEnvDescriptor = Object.getOwnPropertyDescriptor(process, "env");
		let firstRead = true;

		Object.defineProperty(process, "env", {
			configurable: true,
			get() {
				if (firstRead) {
					firstRead = false;
					return undefined;
				}

				return originalEnv;
			},
		});

		try {
			const config = await loadClientUserConfig();
			expect(Object.keys(config.server?.proxy ?? {})).toHaveLength(0);
		} finally {
			if (originalEnvDescriptor) {
				Object.defineProperty(process, "env", originalEnvDescriptor);
			}
		}
	});
});
