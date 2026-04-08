import type { APIEvent } from "@solidjs/start/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { authCookieName } from "~/lib/auth-cookie";

const makeEvent = (url: string, init: RequestInit = {}): APIEvent =>
	({ request: new Request(url, init) }) as APIEvent;

describe("api proxy route", () => {
	beforeEach(() => {
		vi.resetModules();
		vi.restoreAllMocks();
		vi.unstubAllEnvs();
		vi.unstubAllGlobals();
	});

	afterEach(() => {
		vi.restoreAllMocks();
		vi.unstubAllEnvs();
		vi.unstubAllGlobals();
	});

	it("REQ-OPS-015: proxy login stores browser auth in an HttpOnly cookie and redacts the bearer token", async () => {
		vi.stubEnv("BACKEND_URL", "http://localhost:8000");
		const fetchMock = vi.fn().mockResolvedValue(
			new Response(
				JSON.stringify({
					bearer_token: "backend-token",
					user_id: "dev-alice",
					expires_at: 1_900_000_000,
				}),
				{
					headers: {
						"content-type": "application/json",
						"set-cookie": "upstream-session=backend-cookie; Path=/; HttpOnly",
					},
				},
			),
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
		expect(setCookie).not.toContain("upstream-session=");
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

	it("REQ-SEC-003: rejects protocol-relative proxy paths before forwarding browser bearer tokens", async () => {
		vi.stubEnv("BACKEND_URL", "http://127.0.0.1:8000");
		const fetchMock = vi.fn();
		vi.stubGlobal("fetch", fetchMock);
		const { GET } = await import("./[...path]");

		const response = await GET(
			makeEvent("http://127.0.0.1:3000/api//127.0.0.1:9998/browser-steal?z=1", {
				headers: {
					cookie: "ugoite_auth_bearer_token=browser-token",
				},
			}),
		);

		expect(response.status).toBe(400);
		await expect(response.text()).resolves.toBe("Invalid API proxy path");
		expect(fetchMock).not.toHaveBeenCalled();
	});

	it("REQ-SEC-003: pins forwarded browser auth to the configured backend origin", async () => {
		vi.stubEnv("BACKEND_URL", "http://127.0.0.1:8000");
		const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
		vi.stubGlobal("fetch", fetchMock);
		const { GET } = await import("./[...path]");

		const response = await GET(
			makeEvent("http://127.0.0.1:3000/api/spaces?z=1", {
				headers: {
					cookie: "ugoite_auth_bearer_token=browser-token",
				},
			}),
		);

		expect(response.status).toBe(200);
		expect(fetchMock).toHaveBeenCalledTimes(1);
		const [targetUrl, init] = fetchMock.mock.calls[0] as [URL, RequestInit];
		expect(targetUrl.toString()).toBe("http://127.0.0.1:8000/spaces?z=1");
		expect(targetUrl.origin).toBe("http://127.0.0.1:8000");
		const headers = new Headers(init.headers);
		expect(headers.get("authorization")).toBe("Bearer browser-token");
	});

	it("REQ-SEC-003: surfaces malformed BACKEND_URL as server misconfiguration", async () => {
		vi.stubEnv("BACKEND_URL", "http://[");
		const fetchMock = vi.fn();
		vi.stubGlobal("fetch", fetchMock);
		const stderrSpy = vi.spyOn(process.stderr, "write").mockReturnValue(true);
		const { GET } = await import("./[...path]");

		const response = await GET(makeEvent("http://127.0.0.1:3000/api/spaces"));

		expect(response.status).toBe(500);
		await expect(response.text()).resolves.toBe("BACKEND_URL is invalid");
		expect(fetchMock).not.toHaveBeenCalled();
		expect(stderrSpy).toHaveBeenCalledWith(
			expect.stringContaining("API proxy backend misconfiguration"),
		);
	});
});
