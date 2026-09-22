import { expect, test, type APIRequestContext } from "@playwright/test";
import { getBackendUrl, getDefaultSpaceId, waitForServers } from "./lib/client.ts";

test.describe("Form", () => {
	let spaceId = "";

	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		spaceId = await getDefaultSpaceId(request);
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
					const res = await request.post(
						getBackendUrl(`/spaces/${spaceId}/entries/query`),
						{
							data: {
								query: { scope: { kind: "all" }, text: query },
								projection: { kind: "preview" },
								limit: 10,
							},
						},
					);
					if (!res.ok()) return false;
					const page = (await res.json()) as { rows?: Array<{ id?: string }> };
					return (page.rows ?? []).some((entry) => entry.id === entryId);
				},
				{ timeout: 30000 },
			)
			.toBe(true);
	};

	type FormCapability = { id?: string; fields?: Record<string, { query_capability?: { field?: unknown } }> };

	const resolveFormScope = async (
		request: APIRequestContext,
		formName: string,
	): Promise<{ form_id: string; fields: Record<string, { query_capability?: { field?: unknown } }> }> => {
		const listRes = await request.get(getBackendUrl(`/spaces/${spaceId}/forms`));
		expect(listRes.ok()).toBe(true);
		const forms = (await listRes.json()) as Array<FormCapability & { name?: string }>;
		const form = forms.find((candidate) => candidate.name === formName);
		expect(form?.id).toBeTruthy();
		return { form_id: form!.id!, fields: form!.fields ?? {} };
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

		const entryRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/entries`),
			{
				data: {
					form: formName,
					fields: { Status: "Active" },
				},
			},
		);
		expect(entryRes.status()).toBe(201);
		const entry = (await entryRes.json()) as { id: string };
		await waitForForm(request, formName);
		await waitForSearchResult(request, "Active", entry.id);

		const { form_id, fields } = await resolveFormScope(request, formName);
		const statusRef = fields["Status"]?.query_capability?.field;
		expect(statusRef).toBeTruthy();
		const queryRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/entries/query`),
			{
				data: {
					query: { scope: { kind: "form", form_id } },
					projection: { kind: "fields", fields: [statusRef] },
					limit: 100,
				},
			},
		);
		expect(queryRes.ok()).toBe(true);
		const page = (await queryRes.json()) as {
			rows?: Array<{ id?: string; properties?: Record<string, unknown> }>;
		};
		const match = (page.rows ?? []).find((item) => item.id === entry.id);
		expect(match).toBeTruthy();
		expect(Object.values(match?.properties ?? {}).some((value) => value === "Active")).toBe(
			true,
		);

		await request.delete(
			getBackendUrl(`/spaces/${spaceId}/entries/${entry.id}`),
		);
	});

	test("Issue 2138: a newly created Form is immediately queryable", async ({ request }) => {
		const formName = `ImmediateQueryForm-${Date.now()}`;
		const createRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/forms`),
			{
				data: {
					name: formName,
					version: 1,
					template: `# ${formName}\n\n## Status\n`,
					fields: { Status: { type: "string" } },
				},
			},
		);
		expect([200, 201]).toContain(createRes.status());

		const { form_id } = await resolveFormScope(request, formName);
		const queryRes = await request.post(
			getBackendUrl(`/spaces/${spaceId}/entries/query`),
			{
				data: {
					query: { scope: { kind: "form", form_id } },
					projection: { kind: "preview" },
					limit: 100,
				},
			},
		);
		expect(queryRes.status()).toBe(200);
		expect(((await queryRes.json()) as { rows?: unknown[] }).rows).toEqual([]);
	});
});
