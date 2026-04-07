import { describe, expect, it } from "vitest";
import { buildServerAuthCookie, readAuthCookie } from "./auth-cookie";

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
});
