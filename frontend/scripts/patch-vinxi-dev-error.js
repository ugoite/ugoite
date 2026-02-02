// biome-ignore-all lint/correctness/noNodejsModules: This is a script for Node.js
// biome-ignore-all lint/suspicious/noConsole: This is a script for Node.js
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const targetFile = path.resolve(__dirname, "../node_modules/vinxi/lib/dev-error.js");

if (fs.existsSync(targetFile)) {
	let content = fs.readFileSync(targetFile, "utf8");

	// Patch to check if headers are already sent
	const oldCode = `function errorHandler(error, event) {
	setResponseHeader(event, "Content-Type", "text/html; charset=UTF-8");
	setResponseStatus(event, 503, "Server Unavailable");`;

	const newCode = `function errorHandler(error, event) {
	if (event.node.res.headersSent) {
		console.error("[patch-vinxi] Error occurred after headers sent:", error);
		if (!event.node.res.writableEnded) {
			event.node.res.end();
		}
		return;
	}
	setResponseHeader(event, "Content-Type", "text/html; charset=UTF-8");
	setResponseStatus(event, 503, "Server Unavailable");`;

	// Try with tabs first (as seen in cat)
	if (content.includes(oldCode)) {
		content = content.replace(oldCode, newCode);
		fs.writeFileSync(targetFile, content);
		console.log("Successfully patched vinxi/lib/dev-error.js");
	} else {
		// Try with spaces just in case
		const oldCodeSpaces = oldCode.replace(/\t/g, "        ");
		const newCodeSpaces = newCode.replace(/\t/g, "        ");
		if (content.includes(oldCodeSpaces)) {
			content = content.replace(oldCodeSpaces, newCodeSpaces);
			fs.writeFileSync(targetFile, content);
			console.log("Successfully patched vinxi/lib/dev-error.js (spaces)");
		} else if (content.includes("event.node.res.headersSent")) {
			console.log("vinxi/lib/dev-error.js is already patched");
		} else {
			console.warn("Could not find the expected code block in vinxi/lib/dev-error.js to patch");
			// Show actual content for debugging
			console.log("Content starts with:", content.substring(0, 500));
		}
	}
} else {
	console.error("Target file not found:", targetFile);
}
