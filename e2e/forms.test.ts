import { expect, test, type APIRequestContext } from "@playwright/test";
import { getBackendUrl, waitForServers } from "./lib/client";

const spaceId = "default";

test.describe("Form", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
	});

	const waitForForm = async (request: APIRequestContext, formName: string) => {
		await expect
			.poll(
				async () => {
					const res = await request.get(getBackendUrl(`/spaces/${spaceId}/forms`));
					if (!res.ok()) return false;
					const data = (await res.json()) as Array<{ name?: string }>;
					return data.some((form) => form.name === formName);
				},
				{ timeout: 30000 },
			)
			.toBe(true);
	};

	const waitForSearchResult = async (
		request: APIRequestContext,
		query: string,
		entryId: string,
	) => {
		await expect
			.poll(
				async () => {
					const res = await request.get(
						getBackendUrl(`/spaces/${spaceId}/search?q=${encodeURIComponent(query)}`),
					);
					if (!res.ok()) return false;
					const data = (await res.json()) as Array<{ id?: string }>;
					return data.some((entry) => entry.id === entryId);
				},
				{ timeout: 30000 },
			)
			.toBe(true);
	};


	test("Create and List Forms", async ({ request }) => {
		const formName = `E2ETestForm-${Date.now()}`;
		const formDef = {
			name: formName,
			version: 1,
			template: "# E2ETestForm\n\n## Field1\n",
			fields: {
				Field1: { type: "string", required: true },
			},
		};

		const createRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/forms`),
			{ data: formDef },
		);
		expect([200, 201]).toContain(createRes.status());
		await waitForForm(request, formName);
		const listRes = await request.get(getBackendUrl(`/spaces/${spaceId}/forms`));
		expect(listRes.ok()).toBe(true);
		const forms = (await listRes.json()) as Array<{ name?: string }>;
		expect(forms.some((form) => form.name === formName)).toBe(true);
	});

	test("Query Entries by Form", async ({ request }) => {
		const formName = `QueryTestForm-${Date.now()}`;
		const formDef = {
			name: formName,
			version: 1,
			template: "# QueryTestForm\n\n## Status\n",
			fields: {
				Status: { type: "string", required: true },
			},
		};

		await request.post(
			getBackendUrl(`/spaces/${spaceId}/forms`),
			{ data: formDef },
		);

		const entryTitle = `Query Entry ${Date.now()}`;
		const entryContent = `---
form: ${formName}
---
# ${entryTitle}

## Status
Active
`;
		const entryRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/entries`),
			{ data: { content: entryContent } },
		);
		expect(entryRes.status()).toBe(201);
		const entry = (await entryRes.json()) as { id: string };
		await waitForForm(request, formName);
		await waitForSearchResult(request, "Active", entry.id);

		const queryRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/query`),
			{ data: { filter: { form: formName } } },
		);
		expect(queryRes.ok()).toBe(true);
		const entries = (await queryRes.json()) as Array<{
			id?: string;
			title?: string;
			properties?: Record<string, string | null>;
		}>;
		const match = entries.find((item) => item.id === entry.id);
		expect(match).toBeTruthy();
		expect(match?.title).toBe(entryTitle);
		expect(match?.properties?.Status).toBe("Active");

		await request.delete(
			getBackendUrl(`/spaces/${spaceId}/entries/${entry.id}`),
		);
	});
});
