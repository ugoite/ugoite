// REQ-OPS-015: Local dev auth mode selection.
import { beforeEach, describe, expect, it } from "vitest";
import { http, HttpResponse } from "msw";
import { authApi } from "./auth-api";
import { resetMockData, seedDevAuthConfig } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import { testApiUrl } from "~/test/http-origin";

describe("authApi", () => {
	beforeEach(() => {
		resetMockData();
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
						user_id: 42,
						expires_at: 1_900_000_000,
					},
					{ status: 200 },
				),
			),
		);
		await expect(authApi.loginWithPasskeyTotp("dev-alice", "123456")).rejects.toThrow(
			"Invalid auth response: user_id must be a string.",
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

	it("REQ-FE-066: loads and clears browser auth session state through the frontend session route", async () => {
		server.use(
			http.get(testApiUrl("/auth/session"), () =>
				HttpResponse.json({ authenticated: true }, { status: 200 }),
			),
			http.delete(testApiUrl("/auth/session"), () => new HttpResponse(null, { status: 204 })),
		);

		await expect(authApi.getSession()).resolves.toEqual({ authenticated: true });
		await expect(authApi.clearSession()).resolves.toBeUndefined();

		server.use(
			http.get(testApiUrl("/auth/session"), () =>
				HttpResponse.json({ authenticated: "yes" }, { status: 200 }),
			),
		);
		await expect(authApi.getSession()).rejects.toThrow(
			"Invalid auth response: authenticated must be a boolean.",
		);

		server.use(
			http.delete(testApiUrl("/auth/session"), () =>
				HttpResponse.json(
					{ detail: "Failed to clear browser auth session." },
					{ status: 500, statusText: "Internal Server Error" },
				),
			),
		);
		await expect(authApi.clearSession()).rejects.toThrow("Failed to clear browser auth session.");
	});
});
