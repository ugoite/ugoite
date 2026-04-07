// REQ-OPS-015: Local dev auth mode selection.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { authApi } from "./auth-api";
import { clearAuthTokenCookie, hasAuthTokenCookie, setAuthTokenCookie } from "./auth-session";
import { resetMockData, seedDevAuthConfig } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import { testApiUrl } from "~/test/http-origin";

describe("authApi", () => {
	beforeEach(() => {
		resetMockData();
		clearAuthTokenCookie();
	});

	it("REQ-OPS-015: surfaces local auth config and login response errors clearly", async () => {
		seedDevAuthConfig({
			mode: "passkey-totp",
			username_hint: "dev-alice",
			supports_passkey_totp: true,
			supports_mock_oauth: false,
		});

		await expect(authApi.getConfig()).resolves.toEqual({
			mode: "passkey-totp",
			usernameHint: "dev-alice",
			supportsPasskeyTotp: true,
			supportsMockOauth: false,
		});
		await expect(authApi.loginWithPasskeyTotp("dev-alice", "123456")).resolves.toEqual({
			bearerToken: "frontend-test-token",
			userId: "dev-alice",
			expiresAt: 1_900_000_000,
		});
		await expect(authApi.loginWithMockOauth()).rejects.toThrow(
			"mock-oauth login is not enabled for this session.",
		);

		server.use(
			http.get(testApiUrl("/auth/config"), () =>
				HttpResponse.json(
					{
						mode: "passkey-totp",
						username_hint: "dev-alice",
						supports_passkey_totp: "yes",
						supports_mock_oauth: false,
					},
					{ status: 200 },
				),
			),
		);
		await expect(authApi.getConfig()).rejects.toThrow(
			"Invalid auth response: supports_passkey_totp must be a boolean.",
		);

		server.use(
			http.get(
				testApiUrl("/auth/config"),
				() => new HttpResponse(null, { status: 500, statusText: "Internal Server Error" }),
			),
		);
		await expect(authApi.getConfig()).rejects.toThrow(
			"Failed to load auth config: Internal Server Error",
		);

		server.use(
			http.get(testApiUrl("/auth/config"), () =>
				HttpResponse.json(
					{
						detail: "Explicit login endpoints are only available from loopback clients.",
					},
					{ status: 403, statusText: "Forbidden" },
				),
			),
		);
		await expect(authApi.getConfig()).rejects.toThrow(
			"Explicit login endpoints are only available from loopback clients.",
		);

		server.use(
			http.post(testApiUrl("/auth/login"), () =>
				HttpResponse.json(
					{
						bearer_token: 42,
						user_id: "dev-alice",
						expires_at: 1_900_000_000,
					},
					{ status: 200 },
				),
			),
		);
		await expect(authApi.loginWithPasskeyTotp("dev-alice", "123456")).rejects.toThrow(
			"Invalid auth response: bearer_token must be a string.",
		);

		server.use(
			http.post(
				testApiUrl("/auth/login"),
				() => new HttpResponse("backend down", { status: 502, statusText: "Bad Gateway" }),
			),
		);
		await expect(authApi.loginWithPasskeyTotp("dev-alice", "123456")).rejects.toThrow(
			"Failed to log in: Bad Gateway",
		);

		server.use(
			http.post(testApiUrl("/auth/mock-oauth"), () =>
				HttpResponse.json({ detail: { reason: "blocked" } }, { status: 409 }),
			),
		);
		await expect(authApi.loginWithMockOauth()).rejects.toThrow('{"reason":"blocked"}');

		server.use(
			http.post(testApiUrl("/auth/mock-oauth"), () =>
				HttpResponse.json({ detail: 123 }, { status: 409, statusText: "Conflict" }),
			),
		);
		await expect(authApi.loginWithMockOauth()).rejects.toThrow(
			"Failed to start mock OAuth login: Conflict",
		);

		server.use(
			http.post(testApiUrl("/auth/mock-oauth"), () =>
				HttpResponse.json(
					{
						bearer_token: "frontend-test-token",
						user_id: "dev-alice",
						expires_at: "soon",
					},
					{ status: 200 },
				),
			),
		);
		await expect(authApi.loginWithMockOauth()).rejects.toThrow(
			"Invalid auth response: expires_at must be a number.",
		);
	});
});

describe("authSession", () => {
	afterEach(() => {
		vi.unstubAllGlobals();
	});

	it("REQ-OPS-015: writes and clears browser auth cookies with explicit expiry", () => {
		const cookieAssignments: string[] = [];
		const fakeDocument = {
			get cookie() {
				return cookieAssignments.at(-1) ?? "";
			},
			set cookie(value: string) {
				cookieAssignments.push(value);
			},
		};
		vi.stubGlobal("document", fakeDocument);
		vi.stubGlobal("window", { location: { protocol: "https:" } });

		setAuthTokenCookie("token-value", 1_900_000_000);
		setAuthTokenCookie("token-without-expiry");
		clearAuthTokenCookie();

		expect(cookieAssignments[0]).toContain("ugoite_auth_bearer_token=token-value");
		expect(cookieAssignments[0]).toContain("Path=/");
		expect(cookieAssignments[0]).toContain("SameSite=Lax");
		expect(cookieAssignments[0]).toContain("; Secure");
		expect(cookieAssignments[0]).toContain("Max-Age=");
		expect(cookieAssignments[0]).toContain("Expires=");
		expect(cookieAssignments[1]).not.toContain("Max-Age=");
		expect(cookieAssignments[1]).not.toContain("Expires=");
		expect(cookieAssignments[2]).toContain(
			"ugoite_auth_bearer_token=; Path=/; Max-Age=0; SameSite=Lax",
		);
	});

	it("REQ-OPS-015: writes and clears browser auth cookies with explicit expiry safely when document is unavailable", () => {
		vi.stubGlobal("document", undefined);

		expect(() => setAuthTokenCookie("token-value", 1_900_000_000)).not.toThrow();
		expect(() => clearAuthTokenCookie()).not.toThrow();
	});

	it("REQ-OPS-015: no-ops when cookie descriptors are unavailable", () => {
		vi.stubGlobal("document", Object.create(null));
		vi.stubGlobal("window", { location: { protocol: "http:" } });

		expect(() => setAuthTokenCookie("token-value")).not.toThrow();
		expect(() => clearAuthTokenCookie()).not.toThrow();
	});

	it("REQ-FE-066: preserves explicit auth state across browser cookie write timing gaps", () => {
		const cookieAssignments: string[] = [];
		const fakeDocument = {
			get cookie() {
				return "";
			},
			set cookie(value: string) {
				cookieAssignments.push(value);
			},
		};
		vi.stubGlobal("document", fakeDocument);
		vi.stubGlobal("window", { location: { protocol: "http:" }, dispatchEvent: vi.fn() });

		setAuthTokenCookie("token-value", 1_900_000_000);
		expect(cookieAssignments[0]).toContain("ugoite_auth_bearer_token=token-value");
		expect(hasAuthTokenCookie()).toBe(true);

		clearAuthTokenCookie();
		expect(hasAuthTokenCookie()).toBe(false);
	});

	it("REQ-FE-066: drops stale explicit auth state once the browser cookie stays absent", () => {
		vi.useFakeTimers();
		try {
			vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
			const fakeDocument = {
				get cookie() {
					return "";
				},
				set cookie(_value: string) {},
			};
			vi.stubGlobal("document", fakeDocument);
			vi.stubGlobal("window", { location: { protocol: "http:" }, dispatchEvent: vi.fn() });

			setAuthTokenCookie("token-value", 1_900_000_000);
			expect(hasAuthTokenCookie()).toBe(true);

			vi.advanceTimersByTime(1_001);
			expect(hasAuthTokenCookie()).toBe(false);
		} finally {
			vi.useRealTimers();
		}
	});

	it("REQ-FE-066: reports no browser auth cookie when document is unavailable", () => {
		vi.stubGlobal("document", undefined);
		expect(hasAuthTokenCookie()).toBe(false);
	});

	it("REQ-FE-066: falls back to parsing browser cookies when there is no explicit auth state", async () => {
		vi.resetModules();
		const fakeDocument = {
			cookie: "theme=violet; ugoite_auth_bearer_token=browser-session; locale=en",
		};
		vi.stubGlobal("document", fakeDocument);
		const { clearAuthTokenCookie, hasAuthTokenCookie } = await import("./auth-session");

		expect(hasAuthTokenCookie()).toBe(true);

		clearAuthTokenCookie();
		expect(hasAuthTokenCookie()).toBe(false);
	});
});
