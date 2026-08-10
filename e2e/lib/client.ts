import type { APIRequestContext } from "@playwright/test";

const frontendUrl = process.env.FRONTEND_URL ?? "http://localhost:3000";
const backendUrl = process.env.BACKEND_URL ?? "http://localhost:8000";

export function getFrontendUrl(path: string): string {
  return new URL(path, frontendUrl).toString();
}

export function getBackendUrl(path: string): string {
  return new URL(`/api${path}`, frontendUrl).toString();
}

function getDirectBackendUrl(path: string): string {
  return new URL(path, backendUrl).toString();
}

async function waitForOk(
  request: APIRequestContext,
  url: string,
  timeoutMs: number,
): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const response = await request.get(url);
      if (response.ok()) {
        return;
      }
    } catch {
      // Ignore transient errors while waiting.
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(`Timed out waiting for ${url}`);
}

async function waitForReachable(
  request: APIRequestContext,
  url: string,
  timeoutMs: number,
): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const response = await request.get(url);
      if (response.status() < 500) {
        return;
      }
    } catch {
      // Ignore transient errors while waiting.
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(`Timed out waiting for ${url}`);
}

export async function waitForServers(
  request: APIRequestContext,
  options: { timeoutMs?: number } = {},
): Promise<void> {
  const timeoutMs = options.timeoutMs ?? 60_000;
  await waitForReachable(request, getDirectBackendUrl("/spaces"), timeoutMs);
  await waitForOk(request, getFrontendUrl("/"), timeoutMs);
}

export async function ensureDefaultForm(
	request: APIRequestContext,
	spaceId: string,
): Promise<void> {
	const response = await request.post(getBackendUrl(`/spaces/${spaceId}/forms`), {
		data: {
			name: "Entry",
			version: 1,
			template: "# Entry\n\n## Body\n",
			fields: { Body: { type: "markdown", required: false } },
		},
	});
	if (![200, 201, 409].includes(response.status())) {
		const body = await response.text();
		throw new Error(`Failed to ensure default form: ${response.status()} ${body}`);
	}
}

export async function getDefaultFormRelation(
	request: APIRequestContext,
	spaceId: string,
): Promise<string> {
	const response = await request.get(
		getBackendUrl(`/spaces/${spaceId}/forms`),
	);
	if (!response.ok()) {
		throw new Error(`Failed to list default Forms: ${response.status()}`);
	}
	const forms = await response.json() as Array<{
		name?: string;
		sql_relation?: string;
	}>;
	const relation = forms.find((form) => form.name === "Entry")?.sql_relation;
	if (!relation) throw new Error("Default Entry Form has no SQL relation");
	return relation;
}

export async function getDefaultSpaceId(
  request: APIRequestContext,
): Promise<string> {
	// The display name/slug are only fixture-discovery keys. All callers must use
	// the returned immutable ID for subsequent routes and API requests.
	const response = await request.get(getBackendUrl("/spaces"));
	if (!response.ok()) throw new Error(`Failed to list Spaces: ${response.status()}`);
	const spaces = await response.json() as Array<{ id: string; slug?: string; name: string }>;
	const space = spaces.find((candidate) =>
		candidate.name === "default" || candidate.slug === "default"
	);
	if (!space) throw new Error("Default Space was not created during setup");
	return space.id;
}
