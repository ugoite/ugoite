import { spawn, type ChildProcess } from "node:child_process";
import { fileURLToPath } from "node:url";

const docsiteDir = fileURLToPath(new URL("../../docsite", import.meta.url));
const configuredDocsiteUrl = process.env.DOCSITE_URL;

type ReadyOptions = {
	timeoutMs: number;
	getStartupError: () => Error | undefined;
	getProcessExitCode: () => number | null | undefined;
	getRecentLogs: () => string;
};

export type DocsiteServer = {
	buildUrl: (path: string) => string;
	getBaseUrl: () => string;
	stop: () => Promise<void>;
};

export async function startDocsiteServer(options?: {
	basePath?: string;
}): Promise<DocsiteServer> {
	let docsiteProcess: ChildProcess | undefined;
	let docsiteLogs = "";
	let docsiteStartupError: Error | undefined;
	let resolvedDocsiteUrl: string | undefined = configuredDocsiteUrl;

	if (!configuredDocsiteUrl || options?.basePath) {
		resolvedDocsiteUrl = undefined;
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
					...(options?.basePath ? { DOCSITE_BASE: options.basePath } : {}),
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
	}

	return {
		buildUrl: (path: string) => {
			const baseUrl = getResolvedDocsiteUrl(resolvedDocsiteUrl);
			const normalizedBase = baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`;
			const normalizedPath = path.startsWith("/") ? path.slice(1) : path;
			return new URL(normalizedPath, normalizedBase).toString();
		},
		getBaseUrl: () => getResolvedDocsiteUrl(resolvedDocsiteUrl),
		stop: async () => {
			if (!docsiteProcess || docsiteProcess.killed) {
				return;
			}
			docsiteProcess.kill("SIGTERM");
			await new Promise((resolve) => setTimeout(resolve, 800));
			if (!docsiteProcess.killed) {
				docsiteProcess.kill("SIGKILL");
			}
		},
	};
}

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

function getResolvedDocsiteUrl(resolvedDocsiteUrl: string | undefined): string {
	if (!resolvedDocsiteUrl) {
		throw new Error("Resolved docsite URL is unavailable");
	}
	return resolvedDocsiteUrl;
}
