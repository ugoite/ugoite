/**
 * E2E Test Client Library
 *
 * Provides utilities for testing the IEapp frontend and backend
 * using Bun's native fetch API and test runner.
 */

export interface TestConfig {
	frontendUrl: string;
	backendUrl: string;
	timeout: number;
}

export const defaultConfig: TestConfig = {
	frontendUrl: process.env.FRONTEND_URL || "http://localhost:3000",
	backendUrl: process.env.BACKEND_URL || "http://localhost:8000",
	timeout: Number(process.env.E2E_TIMEOUT) || 10000,
};

/**
 * HTTP client for E2E tests with timeout and error handling
 */
export class E2EClient {
	constructor(private config: TestConfig = defaultConfig) {}

	get frontendUrl(): string {
		return this.config.frontendUrl;
	}

	get backendUrl(): string {
		return this.config.backendUrl;
	}

	/**
	 * Make a request with timeout
	 */
	async fetch(
		url: string,
		options: RequestInit = {},
	): Promise<Response> {
		const controller = new AbortController();
		const timeoutId = setTimeout(
			() => controller.abort(),
			this.config.timeout,
		);

		try {
			const response = await fetch(url, {
				...options,
				signal: controller.signal,
			});
			return response;
		} finally {
			clearTimeout(timeoutId);
		}
	}

	/**
	 * GET request to frontend
	 */
	async getFrontend(path: string): Promise<Response> {
		return this.fetch(`${this.config.frontendUrl}${path}`);
	}

	/**
	 * GET request to backend API
	 */
	async getApi(path: string): Promise<Response> {
		return this.fetch(`${this.config.backendUrl}${path}`);
	}

	/**
	 * POST request to backend API
	 */
	async postApi(
		path: string,
		body: unknown,
	): Promise<Response> {
		return this.fetch(`${this.config.backendUrl}${path}`, {
			method: "POST",
			headers: {
				"Content-Type": "application/json",
			},
			body: JSON.stringify(body),
		});
	}

	/**
	 * PUT request to backend API
	 */
	async putApi(
		path: string,
		body: unknown,
	): Promise<Response> {
		return this.fetch(`${this.config.backendUrl}${path}`, {
			method: "PUT",
			headers: {
				"Content-Type": "application/json",
			},
			body: JSON.stringify(body),
		});
	}

	/**
	 * DELETE request to backend API
	 */
	async deleteApi(path: string): Promise<Response> {
		return this.fetch(`${this.config.backendUrl}${path}`, {
			method: "DELETE",
		});
	}
}

/**
 * Assertion helpers for E2E tests
 */
export const assertions = {
	/**
	 * Assert response status code
	 */
	assertStatus(response: Response, expected: number, message?: string): void {
		if (response.status !== expected) {
			throw new Error(
				message ||
					`Expected status ${expected}, got ${response.status}`,
			);
		}
	},

	/**
	 * Assert response is OK (2xx)
	 */
	assertOk(response: Response, message?: string): void {
		if (!response.ok) {
			throw new Error(
				message ||
					`Expected OK response, got ${response.status} ${response.statusText}`,
			);
		}
	},

	/**
	 * Assert response body contains text
	 */
	async assertBodyContains(
		response: Response,
		text: string,
		message?: string,
	): Promise<void> {
		const body = await response.text();
		if (!body.includes(text)) {
			throw new Error(
				message ||
					`Expected body to contain "${text}", but it didn't`,
			);
		}
	},

	/**
	 * Assert JSON response has property
	 */
	async assertJsonHas(
		response: Response,
		property: string,
		expectedValue?: unknown,
	): Promise<void> {
		const json = await response.json();
		if (!(property in json)) {
			throw new Error(`Expected JSON to have property "${property}"`);
		}
		if (expectedValue !== undefined && json[property] !== expectedValue) {
			throw new Error(
				`Expected "${property}" to be ${JSON.stringify(expectedValue)}, got ${JSON.stringify(json[property])}`,
			);
		}
	},
};

/**
 * Wait for a condition to be true
 */
export async function waitFor(
	condition: () => Promise<boolean> | boolean,
	options: { timeout?: number; interval?: number } = {},
): Promise<void> {
	const { timeout = 10000, interval = 100 } = options;
	const start = Date.now();

	while (Date.now() - start < timeout) {
		if (await condition()) {
			return;
		}
		await new Promise((resolve) => setTimeout(resolve, interval));
	}

	throw new Error(`Condition not met within ${timeout}ms`);
}

/**
 * Wait for servers to be ready
 */
export async function waitForServers(
	client: E2EClient,
	options: { timeout?: number } = {},
): Promise<void> {
	const { timeout = 30000 } = options;

	// Wait for backend
	await waitFor(
		async () => {
			try {
				const res = await client.getApi("/api/workspaces");
				return res.ok;
			} catch {
				return false;
			}
		},
		{ timeout, interval: 500 },
	);

	// Wait for frontend
	await waitFor(
		async () => {
			try {
				const res = await client.getFrontend("/");
				return res.ok;
			} catch {
				return false;
			}
		},
		{ timeout, interval: 500 },
	);
}

// Default client instance
export const client = new E2EClient();
