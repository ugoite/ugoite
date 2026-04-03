// REQ-FE-020: Form store management
import { describe, it, expect, beforeEach } from "vitest";
import { createRoot } from "solid-js";
import { http, HttpResponse } from "msw";
import { createFormStore } from "./form-store";
import { resetMockData, seedSpace } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import type { Space } from "./types";
import { testApiUrl } from "~/test/http-origin";

const testSpace: Space = {
	id: "form-store-ws",
	name: "Form Store Space",
	created_at: "2025-01-01T00:00:00Z",
};

describe("createFormStore", () => {
	beforeEach(() => {
		resetMockData();
		seedSpace(testSpace);
	});

	it("loads forms (empty by default)", async () => {
		await createRoot(async (dispose) => {
			const store = createFormStore(() => "form-store-ws");
			expect(store.forms()).toEqual([]);
			await store.loadForms();
			expect(store.loading()).toBe(false);
			expect(store.forms()).toEqual([]);
			dispose();
		});
	});

	it("creates a form and reloads", async () => {
		await createRoot(async (dispose) => {
			const store = createFormStore(() => "form-store-ws");
			const created = await store.createForm({
				name: "Meeting",
				version: 1,
				template: "# Meeting\n",
				fields: { Date: { type: "date", required: true } },
			});
			expect(created.name).toBe("Meeting");
			expect(store.forms()).toHaveLength(1);
			dispose();
		});
	});

	it("gets a single form by name", async () => {
		await createRoot(async (dispose) => {
			const store = createFormStore(() => "form-store-ws");
			await store.createForm({ name: "Task", version: 1, template: "", fields: {} });
			const form = await store.getForm("Task");
			expect(form.name).toBe("Task");
			dispose();
		});
	});

	it("lists form types", async () => {
		await createRoot(async (dispose) => {
			const store = createFormStore(() => "form-store-ws");
			const types = await store.listTypes();
			expect(Array.isArray(types)).toBe(true);
			dispose();
		});
	});

	it("sets error on load failure", async () => {
		server.use(
			http.get(testApiUrl("/spaces/form-store-ws/forms"), () =>
				HttpResponse.json({ detail: "Server error" }, { status: 500 }),
			),
		);
		await createRoot(async (dispose) => {
			const store = createFormStore(() => "form-store-ws");
			await store.loadForms();
			expect(store.error()).toContain("Failed to list forms");
			dispose();
		});
	});
});
