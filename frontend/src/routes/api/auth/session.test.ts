import type { APIEvent } from "@solidjs/start/server";
import { describe, expect, it } from "vitest";

const makeEvent = (url: string, init: RequestInit = {}): APIEvent =>
	({ request: new Request(url, init) }) as APIEvent;

describe("browser auth session route", () => {
	it("REQ-FE-066: reports whether the browser currently has an auth session cookie", async () => {
		const { GET } = await import("./session");

		const withCookie = await GET(
			makeEvent("http://localhost:3000/api/auth/session", {
				headers: {
					cookie: "ugoite_auth_bearer_token=browser-token",
				},
			}),
		);
		await expect(withCookie.json()).resolves.toEqual({ authenticated: true });

		const withoutCookie = await GET(
			makeEvent("http://localhost:3000/api/auth/session", {
				headers: {
					cookie: "theme=violet",
				},
			}),
		);
		await expect(withoutCookie.json()).resolves.toEqual({ authenticated: false });
	});

	it("REQ-FE-066: sign-out clears the browser auth cookie with HttpOnly protection", async () => {
		const { DELETE } = await import("./session");

		const response = await DELETE(
			makeEvent("http://localhost:3000/api/auth/session", {
				method: "DELETE",
				headers: {
					"x-forwarded-proto": "https",
				},
			}),
		);

		expect(response.status).toBe(204);
		const setCookie = response.headers.get("set-cookie");
		expect(setCookie).toContain("ugoite_auth_bearer_token=");
		expect(setCookie).toContain("Path=/");
		expect(setCookie).toContain("HttpOnly");
		expect(setCookie).toContain("SameSite=Lax");
		expect(setCookie).toContain("Max-Age=0");
		expect(setCookie).toContain("Expires=Thu, 01 Jan 1970 00:00:00 GMT");
		expect(setCookie).toContain("Secure");
	});
});
