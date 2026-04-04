import { afterEach, describe, expect, it, vi } from "vitest";
import { testApiUrl } from "~/test/http-origin";
import { resetMockData, seedDevAuthConfig } from "./handlers";

describe("MSW handlers", () => {
	afterEach(() => {
		vi.unstubAllEnvs();
	});

	it("REQ-OPS-015: MSW handlers honor FRONTEND_TEST_ORIGIN", async () => {
		vi.stubEnv("FRONTEND_TEST_ORIGIN", "http://127.0.0.1:4310");
		resetMockData();
		seedDevAuthConfig({
			mode: "mock-oauth",
			username_hint: "dev-oauth-user",
			supports_passkey_totp: false,
			supports_mock_oauth: true,
		});

		const response = await fetch(testApiUrl("/auth/config"));
		const payload = (await response.json()) as { mode: string; username_hint: string };

		expect(response.ok).toBe(true);
		expect(payload).toMatchObject({
			mode: "mock-oauth",
			username_hint: "dev-oauth-user",
		});
	});
});
