import { setResponseHeader, setResponseStatus } from "vinxi/http";

/**
 * Fundamental solution for SolidStart/Vinxi ERR_HTTP_HEADERS_SENT
 * This error handler checks if headers have already been sent before attempting to set them.
 */
// biome-ignore lint/style/noDefaultExport: Nitro requires default export for error handler
// biome-ignore lint/suspicious/noExplicitAny: handler for Nitro/Vinxi
export default function errorHandler(error: any, event: any) {
	// 1. Check if headers are already sent
	if (event.node?.res?.headersSent || event.res?.headersSent) {
		// biome-ignore lint/suspicious/noConsole: Error handler should log to console
		console.error("[ieapp-error-handler] Error occurring after headers sent:", error);
		if (event.node?.res && !event.node.res.writableEnded) {
			event.node.res.end();
		}
		return;
	}

	// 2. Clear out any state if possible (though we can't easily reset headers)
	// biome-ignore lint/suspicious/noConsole: Error handler should log to console
	console.error("[ieapp-error-handler] Server Error:", error);

	try {
		setResponseStatus(event, 503, "Server Unavailable");
		setResponseHeader(event, "Content-Type", "text/html; charset=UTF-8");
		setResponseHeader(event, "Cache-Control", "no-cache, no-store, must-revalidate");

		const isDev = process.env.NODE_ENV === "development";
		const stack = isDev
			? `<pre style="padding: 1rem; background: #eee; overflow: auto;">${error?.stack || error}</pre>`
			: "";

		event.node.res.end(`<!DOCTYPE html>
<html>
<head>
    <title>Server Error</title>
    <style>body { font-family: system-ui; padding: 2rem; }</style>
</head>
<body>
    <h1>Internal Server Error</h1>
    <p>Something went wrong on the server.</p>
    ${stack}
</body>
</html>`);
	} catch (e) {
		// biome-ignore lint/suspicious/noConsole: Error handler should log to console
		console.error("[ieapp-error-handler] Fatal error in error handler:", e);
		if (event.node?.res && !event.node.res.writableEnded) {
			event.node.res.end("Internal Server Error");
		}
	}
}
