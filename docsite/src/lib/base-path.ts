const rawBaseUrl = import.meta.env.BASE_URL ?? "/";

const normalizedBaseUrl = rawBaseUrl.endsWith("/")
	? rawBaseUrl
	: `${rawBaseUrl}/`;

const normalizedBaseNoTrailingSlash = normalizedBaseUrl.endsWith("/")
	? normalizedBaseUrl.slice(0, -1)
	: normalizedBaseUrl;

export function withBasePath(href: string): string {
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
