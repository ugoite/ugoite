import type { APIEvent } from "@solidjs/start/server";
import { buildClearedAuthCookie, readAuthCookie } from "~/lib/auth-cookie";

const requestUsesHttps = (request: Request): boolean => {
	const forwardedProto = request.headers.get("x-forwarded-proto");
	const effectiveProto = forwardedProto?.split(",", 1)[0]?.trim().toLowerCase();
	if (effectiveProto) {
		return effectiveProto === "https";
	}
	return new URL(request.url).protocol === "https:";
};

export const GET = (event: APIEvent): Response =>
	Response.json({
		authenticated: readAuthCookie(event.request.headers.get("cookie")) !== null,
	});

export const DELETE = (event: APIEvent): Response =>
	new Response(null, {
		status: 204,
		headers: {
			"set-cookie": buildClearedAuthCookie({ secure: requestUsesHttps(event.request) }),
		},
	});
