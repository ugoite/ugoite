// REQ-OPS-015: Local dev auth mode selection.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { server } from "~/test/mocks/server";

const getRequestEventMock = vi.fn();

vi.mock("solid-js/web", () => ({
	getRequestEvent: getRequestEventMock,
}));

describe("apiFetch auth forwarding", () => {
	beforeEach(() => {
		getRequestEventMock.mockReset();
		vi.resetModules();
	});

	afterEach(() => {
		vi.unstubAllGlobals();
		vi.unstubAllEnvs();
	});

	it("REQ-OPS-015: forwards SSR auth headers to local auth requests", async () => {
		let seenCookie: string | null = null;
		let seenAuthorization: string | null = null;
		server.use(
			http.get("http://localhost:3000/api/auth/config", ({ request }) => {
				seenCookie = request.headers.get("cookie");
				seenAuthorization = request.headers.get("authorization");
				return HttpResponse.json({
					mode: "passkey-totp",
					username_hint: "dev-alice",
					supports_passkey_totp: true,
					supports_mock_oauth: false,
				});
			}),
		);
		vi.stubGlobal("window", undefined);
		getRequestEventMock.mockReturnValue({
			request: new Request("http://localhost:3000/login", {
				headers: {
					cookie: "ugoite_auth_bearer_token=server-token",
					authorization: "Bearer forwarded-token",
				},
			}),
		});

		const { apiFetch } = await import("./api");
		const response = await apiFetch("/auth/config", { trackLoading: false });

		expect(response.status).toBe(200);
		expect(seenCookie).toBe("ugoite_auth_bearer_token=server-token");
		expect(seenAuthorization).toBe("Bearer forwarded-token");
	});

	it("REQ-OPS-015: preserves explicit request auth headers during SSR", async () => {
		let seenCookie: string | null = null;
		let seenAuthorization: string | null = null;
		server.use(
			http.get("http://localhost:3000/api/auth/config", ({ request }) => {
				seenCookie = request.headers.get("cookie");
				seenAuthorization = request.headers.get("authorization");
				return HttpResponse.json({
					mode: "passkey-totp",
					username_hint: "dev-alice",
					supports_passkey_totp: true,
					supports_mock_oauth: false,
				});
			}),
		);
		vi.stubGlobal("window", undefined);
		getRequestEventMock.mockReturnValue({
			request: new Request("http://localhost:3000/login", {
				headers: {
					cookie: "ugoite_auth_bearer_token=server-token",
					authorization: "Bearer forwarded-token",
				},
			}),
		});

		const { apiFetch } = await import("./api");
		const response = await apiFetch("/auth/config", {
			trackLoading: false,
			headers: {
				cookie: "ugoite_auth_bearer_token=explicit-token",
				authorization: "Bearer explicit-token",
			},
		});

		expect(response.status).toBe(200);
		expect(seenCookie).toBe("ugoite_auth_bearer_token=explicit-token");
		expect(seenAuthorization).toBe("Bearer explicit-token");
	});

	it("REQ-OPS-015: skips SSR auth forwarding without a request event", async () => {
		let seenCookie: string | null = "unexpected";
		let seenAuthorization: string | null = "unexpected";
		server.use(
			http.get("http://localhost:3000/api/auth/config", ({ request }) => {
				seenCookie = request.headers.get("cookie");
				seenAuthorization = request.headers.get("authorization");
				return HttpResponse.json({
					mode: "passkey-totp",
					username_hint: "dev-alice",
					supports_passkey_totp: true,
					supports_mock_oauth: false,
				});
			}),
		);
		vi.stubGlobal("window", undefined);
		getRequestEventMock.mockReturnValue(undefined);

		const { apiFetch } = await import("./api");
		const response = await apiFetch("/auth/config", { trackLoading: false });

		expect(response.status).toBe(200);
		expect(seenCookie).toBeNull();
		expect(seenAuthorization).toBeNull();
	});

	it("REQ-OPS-015: derives the SSR API origin from the incoming request when FRONTEND_ORIGIN is unset", async () => {
		let seenOrigin: string | null = null;
		server.use(
			http.get("http://localhost:13000/api/auth/config", ({ request }) => {
				seenOrigin = new URL(request.url).origin;
				return HttpResponse.json({
					mode: "passkey-totp",
					username_hint: "dev-alice",
					supports_passkey_totp: true,
					supports_mock_oauth: false,
				});
			}),
		);
		vi.stubGlobal("window", undefined);
		vi.stubEnv("NODE_ENV", "development");
		vi.stubEnv("FRONTEND_ORIGIN", "");
		vi.stubEnv("ORIGIN", "");
		getRequestEventMock.mockReturnValue({
			request: new Request("http://localhost:13000/login", {
				headers: {
					cookie: "ugoite_auth_bearer_token=server-token",
				},
			}),
		});

		const { apiFetch } = await import("./api");
		const response = await apiFetch("/auth/config", { trackLoading: false });

		expect(response.status).toBe(200);
		expect(seenOrigin).toBe("http://localhost:13000");
	});

	it("REQ-OPS-015: falls back to the default SSR origin when no request event is available", async () => {
		vi.stubGlobal("window", undefined);
		vi.stubEnv("NODE_ENV", "development");
		vi.stubEnv("FRONTEND_ORIGIN", "");
		vi.stubEnv("ORIGIN", "");
		getRequestEventMock.mockReturnValue(undefined);

		const { getBackendBase } = await import("./api");

		expect(getBackendBase()).toBe("http://localhost:3000/api");
	});

	it("REQ-OPS-015: ignores malformed SSR request URLs when deriving the API origin", async () => {
		vi.stubGlobal("window", undefined);
		vi.stubEnv("NODE_ENV", "development");
		vi.stubEnv("FRONTEND_ORIGIN", "");
		vi.stubEnv("ORIGIN", "");
		getRequestEventMock.mockReturnValue({
			request: {
				url: "::not-a-url::",
			},
		});

		const { getBackendBase } = await import("./api");

		expect(getBackendBase()).toBe("http://localhost:3000/api");
	});
});
