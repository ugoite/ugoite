function normalizeBaseUrl(baseUrl: string): string {
	const trimmedBaseUrl = baseUrl.trim();
	if (trimmedBaseUrl.length === 0 || trimmedBaseUrl === "/") {
		return "/";
	}

	const withLeadingSlash = trimmedBaseUrl.startsWith("/")
		? trimmedBaseUrl
		: `/${trimmedBaseUrl}`;
	return withLeadingSlash.endsWith("/")
		? withLeadingSlash
		: `${withLeadingSlash}/`;
}

export function withBasePath(
	href: string,
	baseUrl: string = import.meta.env.BASE_URL,
): string {
	const normalizedBaseUrl = normalizeBaseUrl(baseUrl);
	const normalizedBaseNoTrailingSlash =
		normalizedBaseUrl === "/" ? "/" : normalizedBaseUrl.slice(0, -1);

	if (href.length === 0) {
		return normalizedBaseUrl;
	}

	if (
		href.startsWith("#") ||
		href.startsWith("?") ||
		href.startsWith("//") ||
		/^[a-zA-Z][a-zA-Z\d+.-]*:/.test(href)
	) {
		return href;
	}

	if (
		normalizedBaseUrl !== "/" &&
		(href === normalizedBaseNoTrailingSlash ||
			href.startsWith(`${normalizedBaseNoTrailingSlash}/`))
	) {
		return href;
	}

	const normalizedPath = href.startsWith("/") ? href.slice(1) : href;
	if (normalizedPath.length === 0) {
		return normalizedBaseUrl;
	}

	return normalizedBaseUrl === "/"
		? `/${normalizedPath}`
		: `${normalizedBaseUrl}${normalizedPath}`;
}
