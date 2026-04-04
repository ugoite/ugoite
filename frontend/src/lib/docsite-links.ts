const defaultLocalDocsiteOrigin = "http://localhost:4321";
const defaultPublishedDocsiteOrigin = "https://ugoite.github.io/ugoite";
const repoBlobBaseUrl = "https://github.com/ugoite/ugoite/blob/main";

const normalizeOrigin = (origin: string): string => origin.replace(/\/$/, "");

const normalizePath = (pathname: string): string => {
	if (!pathname.startsWith("/")) {
		return `/${pathname}`;
	}
	return pathname;
};

const getEnvMap = (): Record<string, string | undefined> =>
	typeof process !== "undefined" && process.env
		? (process.env as Record<string, string | undefined>)
		: {};

export const getDocsiteHref = (
	docsitePath: string,
	githubFallbackPath?: string,
	env = getEnvMap(),
): string => {
	const configuredOrigin = env.DOCSITE_ORIGIN?.trim();
	const origin =
		configuredOrigin !== undefined
			? configuredOrigin
			: env.NODE_ENV === "development"
				? defaultLocalDocsiteOrigin
				: defaultPublishedDocsiteOrigin;

	if (origin.length > 0) {
		return `${normalizeOrigin(origin)}${normalizePath(docsitePath)}`;
	}

	if (!githubFallbackPath) {
		return normalizePath(docsitePath);
	}

	return `${repoBlobBaseUrl}/${githubFallbackPath.replace(/^\/+/, "")}`;
};
