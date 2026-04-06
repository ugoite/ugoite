import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { APIEvent } from "@solidjs/start/server";
import { authCookieName } from "~/lib/auth-cookie";

const makeEvent = (url: string, init?: RequestInit): APIEvent =>
	({
		request: new Request(url, init),
	}) as APIEvent;

describe("/api/[...path]", () => {
	beforeEach(() => {
		vi.resetModules();
	});

	afterEach(() => {
		vi.unstubAllGlobals();
		vi.unstubAllEnvs();
	});

	it("REQ-OPS-015: proxy login stores browser auth in an HttpOnly cookie and redacts the bearer token", async () => {
		vi.stubEnv("BACKEND_URL", "http://localhost:8000");
		const fetchMock = vi.fn().mockResolvedValue(
			Response.json({
				bearer_token: "backend-token",
				user_id: "dev-alice",
				expires_at: 1_900_000_000,
			}),
		);
		vi.stubGlobal("fetch", fetchMock);

		const { POST } = await import("./[...path]");
		const response = await POST(
			makeEvent("http://localhost:3000/api/auth/mock-oauth", { method: "POST" }),
		);

		expect(String(fetchMock.mock.calls[0]?.[0])).toBe("http://localhost:8000/auth/mock-oauth");
		const payload = (await response.json()) as Record<string, unknown>;
		expect(payload).toEqual({
			user_id: "dev-alice",
			expires_at: 1_900_000_000,
		});
		expect(JSON.stringify(payload)).not.toContain("backend-token");
		const setCookie = response.headers.get("set-cookie");
		expect(setCookie).toContain(`${authCookieName}=backend-token`);
		expect(setCookie).toContain("Path=/");
		expect(setCookie).toContain("HttpOnly");
		expect(setCookie).toContain("SameSite=Lax");
		expect(setCookie).toContain("Max-Age=");
		expect(setCookie).toContain("Expires=");
		expect(setCookie).not.toContain("Secure");
	});

	it("REQ-OPS-015: proxy login marks auth cookies Secure when HTTPS is forwarded", async () => {
		vi.stubEnv("BACKEND_URL", "http://localhost:8000");
		const fetchMock = vi.fn().mockResolvedValue(
			Response.json({
				bearer_token: "backend-token",
				user_id: "dev-alice",
				expires_at: 1_900_000_000,
			}),
		);
		vi.stubGlobal("fetch", fetchMock);

		const { POST } = await import("./[...path]");
		const response = await POST(
			makeEvent("http://localhost:3000/api/auth/login", {
				method: "POST",
				headers: {
					"x-forwarded-proto": "https",
				},
				body: JSON.stringify({ username: "dev-alice", totp_code: "123456" }),
			}),
		);

		expect(response.headers.get("set-cookie")).toContain("Secure");
	});
});
