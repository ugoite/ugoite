import { expect, test } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import { fileURLToPath } from "node:url";

const docsiteDir = fileURLToPath(new URL("../docsite", import.meta.url));

const configuredDocsiteUrl = process.env.DOCSITE_URL;
const docPath = "/docs/spec/index";

let docsiteProcess: ChildProcess | undefined;
let docsiteLogs = "";
let docsiteStartupError: Error | undefined;
let resolvedDocsiteUrl: string | undefined = configuredDocsiteUrl;

test.describe("Docsite mobile navigation", () => {
	test.beforeAll(async () => {
		test.setTimeout(180_000);

		docsiteLogs = "";
		docsiteStartupError = undefined;
		resolvedDocsiteUrl = configuredDocsiteUrl;

		docsiteProcess = spawn(
			"bun",
			[
				"run",
				"dev",
				"--port",
				"0",
			],
			{
				cwd: docsiteDir,
				stdio: "pipe",
				env: {
					...process.env,
				},
			},
		);

		docsiteProcess.stdout?.on("data", (chunk) => {
			const text = chunk.toString();
			docsiteLogs += text;
			const urlMatches = text.match(
				/http:\/\/(localhost|127\.0\.0\.1):\d+(?:\/[A-Za-z0-9._~!$&'()*+,;=:@%/-]*)?/g,
			);
			if (urlMatches && urlMatches.length > 0) {
				const lastUrl = urlMatches[urlMatches.length - 1];
				resolvedDocsiteUrl = lastUrl.endsWith("/") ? lastUrl : `${lastUrl}/`;
			}
			if (docsiteLogs.length > 20_000) {
				docsiteLogs = docsiteLogs.slice(-20_000);
			}
		});

		docsiteProcess.stderr?.on("data", (chunk) => {
			docsiteLogs += chunk.toString();
			if (docsiteLogs.length > 20_000) {
				docsiteLogs = docsiteLogs.slice(-20_000);
			}
		});

		docsiteProcess.on("error", (error) => {
			docsiteStartupError = error;
		});

		docsiteProcess.on("exit", (code) => {
			if (code && code !== 0) {
				console.warn(`docsite dev server exited with code ${code}`);
			}
		});

		await waitForDocsiteReady(() => resolvedDocsiteUrl, {
			timeoutMs: 120_000,
			getStartupError: () => docsiteStartupError,
			getProcessExitCode: () => docsiteProcess?.exitCode,
			getRecentLogs: () => docsiteLogs,
		});
	});

	test.afterAll(async () => {
		if (!docsiteProcess || docsiteProcess.killed) {
			return;
		}
		docsiteProcess.kill("SIGTERM");
		await new Promise((resolve) => setTimeout(resolve, 800));
		if (!docsiteProcess.killed) {
			docsiteProcess.kill("SIGKILL");
		}
	});

	test("REQ-E2E-005: mobile nav opens as drawer and keeps reading area uncluttered", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 390, height: 844 });
		await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

		await expect(page.locator(".mobile-nav-toggle")).toBeVisible();
		await expect(page.locator(".site-nav")).toBeHidden();
		await expect(page.locator("main .doc-sidebar").first()).toBeHidden();

		await page.locator(".mobile-nav-toggle").click();
		await expect(page.locator(".mobile-doc-nav")).toHaveClass(/is-open/);
		await expect(page.locator(".mobile-nav-overlay")).toBeVisible();
		await expect(page.locator(".mobile-doc-nav .doc-sidebar-link").first()).toBeVisible();
		await expect(page.locator("body")).toHaveClass(/mobile-nav-open/);
	});

	test("REQ-E2E-005: mobile nav closes by escape and link tap", async ({ page }) => {
		await page.setViewportSize({ width: 390, height: 844 });
		await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

		await page.locator(".mobile-nav-toggle").click();
		await expect(page.locator(".mobile-doc-nav")).toHaveClass(/is-open/);
		await page.keyboard.press("Escape");
		await expect(page.locator(".mobile-doc-nav")).not.toHaveClass(/is-open/);

		await page.locator(".mobile-nav-toggle").click();
		await expect(page.locator(".mobile-doc-nav")).toHaveClass(/is-open/);
		await page.locator(".mobile-doc-nav .doc-sidebar-link").first().click();
		await expect(page.locator(".mobile-doc-nav")).not.toHaveClass(/is-open/);
	});

	test("REQ-E2E-005: mobile nav closes by overlay click", async ({ page }) => {
		await page.setViewportSize({ width: 390, height: 844 });
		await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

		await page.locator(".mobile-nav-toggle").click();
		await expect(page.locator(".mobile-doc-nav")).toHaveClass(/is-open/);
		await expect(page.locator(".mobile-nav-overlay")).toBeVisible();

		await page.locator(".mobile-nav-overlay").click();
		await expect(page.locator(".mobile-doc-nav")).not.toHaveClass(/is-open/);
	});
});

type ReadyOptions = {
	timeoutMs: number;
	getStartupError: () => Error | undefined;
	getProcessExitCode: () => number | null | undefined;
	getRecentLogs: () => string;
};

async function waitForDocsiteReady(
	getUrl: () => string | undefined,
	options: ReadyOptions,
): Promise<void> {
	const started = Date.now();
	while (Date.now() - started < options.timeoutMs) {
		const startupError = options.getStartupError();
		if (startupError) {
			throw startupError;
		}

		const exitCode = options.getProcessExitCode();
		if (exitCode !== null && exitCode !== undefined) {
			throw new Error(
				`Docsite server exited before becoming ready (code=${exitCode}).\n${options.getRecentLogs()}`,
			);
		}

		try {
			const url = getUrl();
			if (!url) {
				await new Promise((resolve) => setTimeout(resolve, 500));
				continue;
			}
			const response = await fetch(url);
			if (response.ok) {
				return;
			}
		} catch {
			// wait for server boot
		}
		await new Promise((resolve) => setTimeout(resolve, 500));
	}
	throw new Error(`Timed out waiting for docsite server at ${getUrl() ?? "<unknown>"}.\n${options.getRecentLogs()}`);
}

function getResolvedDocsiteUrl(): string {
	if (!resolvedDocsiteUrl) {
		throw new Error("Resolved docsite URL is unavailable");
	}
	return resolvedDocsiteUrl;
}

function buildDocsiteUrl(path: string): string {
	const baseUrl = getResolvedDocsiteUrl();
	const normalizedBase = baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`;
	const normalizedPath = path.startsWith("/") ? path.slice(1) : path;
	return new URL(normalizedPath, normalizedBase).toString();
}
