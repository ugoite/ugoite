import { describe, expect, it, vi } from "vitest";
import { buildClearedAuthCookie, buildServerAuthCookie, readAuthCookie } from "./auth-cookie";

describe("authCookie", () => {
	it("REQ-OPS-015: reads auth cookies from browser request headers", () => {
		expect(readAuthCookie("theme=dark; ugoite_auth_bearer_token=token%20value")).toBe(
			"token value",
		);
	});

	it("REQ-OPS-015: treats missing, blank, and undecodable auth cookies safely", () => {
		expect(readAuthCookie(null)).toBeNull();
		expect(readAuthCookie("ugoite_auth_bearer_token=")).toBeNull();
		expect(readAuthCookie("ugoite_auth_bearer_token=%E0%A4%A")).toBe("%E0%A4%A");
		expect(readAuthCookie("theme=dark")).toBeNull();
	});

	it("REQ-OPS-015: builds HttpOnly auth cookies with explicit expiry", () => {
		const expiresAt = 1_900_000_000;
		const header = buildServerAuthCookie("token value", expiresAt, {
			secure: true,
			nowMs: (expiresAt - 60) * 1000,
		});

		expect(header).toContain("ugoite_auth_bearer_token=token%20value");
		expect(header).toContain("Path=/");
		expect(header).toContain("HttpOnly");
		expect(header).toContain("SameSite=Lax");
		expect(header).toContain("Secure");
		expect(header).toContain("Max-Age=60");
		expect(header).toContain(`Expires=${new Date(expiresAt * 1000).toUTCString()}`);
	});

	it("REQ-OPS-015: omits auth cookie expiry when the browser session should stay transient", () => {
		const header = buildServerAuthCookie("token-value", undefined, { secure: false });

		expect(header).toContain("ugoite_auth_bearer_token=token-value");
		expect(header).not.toContain("Secure");
		expect(header).not.toContain("Max-Age=");
		expect(header).not.toContain("Expires=");
	});

	it("REQ-OPS-015: falls back to Date.now when the browser auth cookie expiry does not provide nowMs", () => {
		const dateNowSpy = vi.spyOn(Date, "now").mockReturnValue(1_899_999_940_000);
		try {
			const header = buildServerAuthCookie("token-value", 1_900_000_000, { secure: false });

			expect(header).toContain("Max-Age=60");
			expect(header).toContain(`Expires=${new Date(1_900_000_000 * 1000).toUTCString()}`);
		} finally {
			dateNowSpy.mockRestore();
		}
	});

	it("REQ-FE-066: builds an HttpOnly clearing cookie for browser sign-out", () => {
		const header = buildClearedAuthCookie({ secure: true });

		expect(header).toContain("ugoite_auth_bearer_token=");
		expect(header).toContain("Path=/");
		expect(header).toContain("HttpOnly");
		expect(header).toContain("SameSite=Lax");
		expect(header).toContain("Max-Age=0");
		expect(header).toContain("Expires=Thu, 01 Jan 1970 00:00:00 GMT");
		expect(header).toContain("Secure");
	});

	it("REQ-FE-066: omits Secure when clearing browser auth cookies over plain HTTP", () => {
		const header = buildClearedAuthCookie({ secure: false });

		expect(header).not.toContain("Secure");
	});
});
