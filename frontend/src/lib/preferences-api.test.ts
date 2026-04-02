// REQ-FE-044: Portable locale preferences surface explicit API errors
// REQ-FE-059: Portable theme preferences surface explicit API errors
import { beforeEach, describe, expect, it } from "vitest";
import { http, HttpResponse } from "msw";
import { preferencesApi } from "./preferences-api";
import { resetMockData } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import { testApiUrl } from "~/test/http-origin";

describe("preferencesApi", () => {
	beforeEach(() => {
		resetMockData();
	});

	it("REQ-FE-044: surfaces string detail on portable preference load failure", async () => {
		server.use(
			http.get(testApiUrl("/preferences/me"), () =>
				HttpResponse.json({ detail: "Load failed" }, { status: 500 }),
			),
		);

		await expect(preferencesApi.getMe()).rejects.toThrow("Load failed");
	});

	it("REQ-FE-059: stringifies structured detail on portable preference update failure", async () => {
		server.use(
			http.patch(testApiUrl("/preferences/me"), () =>
				HttpResponse.json({ detail: { field: "ui_theme" } }, { status: 422 }),
			),
		);

		await expect(preferencesApi.patchMe({ ui_theme: "classic" })).rejects.toThrow(
			JSON.stringify({ field: "ui_theme" }),
		);
	});

	it("REQ-FE-059: falls back to status text when portable preference errors are not JSON", async () => {
		server.use(
			http.patch(
				testApiUrl("/preferences/me"),
				() => new HttpResponse("boom", { status: 500, statusText: "Server Error" }),
			),
		);

		await expect(preferencesApi.patchMe({ color_mode: "dark" })).rejects.toThrow(
			"Failed to update preferences: Server Error",
		);
	});

	it("REQ-FE-059: falls back to status text when portable preference detail is empty", async () => {
		server.use(
			http.patch(testApiUrl("/preferences/me"), () =>
				HttpResponse.json({ detail: "" }, { status: 500, statusText: "Server Error" }),
			),
		);

		await expect(preferencesApi.patchMe({ ui_theme: "pop" })).rejects.toThrow(
			"Failed to update preferences: Server Error",
		);
	});
});
