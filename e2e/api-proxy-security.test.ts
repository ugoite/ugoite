import { createServer } from "node:http";
import { expect, test } from "@playwright/test";

test("REQ-SEC-003: browser /api proxy rejects protocol-relative upstream targets", async ({
	page,
}) => {
	let seenAuthorization: string | null = null;
	const captureServer = createServer((request, response) => {
		seenAuthorization = request.headers.authorization ?? null;
		response.writeHead(200, { "content-type": "text/plain" });
		response.end("capture");
	});

	await new Promise<void>((resolve, reject) => {
		captureServer.listen(0, "127.0.0.1", () => resolve());
		captureServer.once("error", reject);
	});

	const address = captureServer.address();
	if (!address || typeof address === "string") {
		throw new Error("capture server failed to bind to a TCP port");
	}

	try {
		await page.context().addCookies([
			{
				name: "ugoite_auth_bearer_token",
				value: "browser-token",
				url: "http://localhost:3000",
			},
		]);
		const response = await page.request.get(
			`/api//127.0.0.1:${address.port}/browser-steal?z=1`,
		);

		expect(response.status()).toBe(400);
		await expect(response.text()).resolves.toContain("Invalid API proxy path");
		expect(seenAuthorization).toBeNull();
	} finally {
		await new Promise<void>((resolve, reject) => {
			captureServer.close((error) => (error ? reject(error) : resolve()));
		});
	}
});
