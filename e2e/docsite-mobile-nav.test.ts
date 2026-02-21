import { expect, test } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import { createServer } from "node:net";
import { fileURLToPath } from "node:url";

const docsiteDir = fileURLToPath(new URL("../docsite", import.meta.url));

let docsitePort = Number(process.env.DOCSITE_PORT ?? "0");
let docsiteUrl = process.env.DOCSITE_URL ?? `http://localhost:${docsitePort}`;
const docPath = "/docs/spec/index";

let docsiteProcess: ChildProcess | undefined;

test.describe("Docsite mobile navigation", () => {
	test.beforeAll(async () => {
		docsitePort = docsitePort > 0 ? docsitePort : await findAvailablePort();
		if (docsitePort <= 0) {
			throw new Error("Unable to allocate port for docsite test server");
		}
		docsiteUrl = process.env.DOCSITE_URL ?? `http://localhost:${docsitePort}`;

		docsiteProcess = spawn(
			"bun",
			["run", "dev", "--port", String(docsitePort), "--strictPort"],
			{
				cwd: docsiteDir,
				stdio: "pipe",
				env: {
					...process.env,
				},
			},
		);

		docsiteProcess.on("exit", (code) => {
			if (code && code !== 0) {
				console.warn(`docsite dev server exited with code ${code}`);
			}
		});

		await waitForDocsiteReady(docsiteUrl);
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
		await page.goto(`${docsiteUrl}${docPath}`, { waitUntil: "networkidle" });

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
		await page.goto(`${docsiteUrl}${docPath}`, { waitUntil: "networkidle" });

		await page.locator(".mobile-nav-toggle").click();
		await expect(page.locator(".mobile-doc-nav")).toHaveClass(/is-open/);
		await page.keyboard.press("Escape");
		await expect(page.locator(".mobile-doc-nav")).not.toHaveClass(/is-open/);

		await page.locator(".mobile-nav-toggle").click();
		await expect(page.locator(".mobile-doc-nav")).toHaveClass(/is-open/);
		await page.locator(".mobile-doc-nav .doc-sidebar-link").first().click();
		await expect(page.locator(".mobile-doc-nav")).not.toHaveClass(/is-open/);
	});
});

async function waitForDocsiteReady(url: string, timeoutMs = 60_000): Promise<void> {
	const started = Date.now();
	while (Date.now() - started < timeoutMs) {
		try {
			const response = await fetch(url);
			if (response.ok) {
				return;
			}
		} catch {
			// wait for server boot
		}
		await new Promise((resolve) => setTimeout(resolve, 500));
	}
	throw new Error(`Timed out waiting for docsite server at ${url}`);
}

async function findAvailablePort(): Promise<number> {
	return await new Promise((resolve) => {
		const server = createServer();
		server.once("error", () => resolve(0));
		server.once("listening", () => {
			const address = server.address();
			const resolvedPort = typeof address === "object" && address ? address.port : 0;
			server.close(() => resolve(resolvedPort));
		});
		server.listen(0);
	});
}
