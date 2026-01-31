import type { APIRequestContext } from "@playwright/test";

const frontendUrl = process.env.FRONTEND_URL ?? "http://localhost:3000";
const backendUrl = process.env.BACKEND_URL ?? "http://localhost:8000";

export function getFrontendUrl(path: string): string {
	return new URL(path, frontendUrl).toString();
}

export function getBackendUrl(path: string): string {
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

export async function waitForServers(
	request: APIRequestContext,
	options: { timeoutMs?: number } = {},
): Promise<void> {
	const timeoutMs = options.timeoutMs ?? 60_000;
	await waitForOk(request, getBackendUrl("/workspaces"), timeoutMs);
	await waitForOk(request, getFrontendUrl("/"), timeoutMs);
}

export async function ensureDefaultClass(
	request: APIRequestContext,
): Promise<void> {
	const response = await request.post(getBackendUrl("/workspaces/default/classes"), {
		data: {
			name: "Note",
			version: 1,
			template: "# Note\n\n## Body\n",
			fields: { Body: { type: "markdown", required: false } },
		},
	});
	if (![200, 201, 409].includes(response.status())) {
		const body = await response.text();
		throw new Error(`Failed to ensure default class: ${response.status()} ${body}`);
	}
}

