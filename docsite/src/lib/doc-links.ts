import { existsSync, statSync } from "node:fs";
import path from "node:path";
import { withBasePath } from "./base-path";

const docsRoot = path.resolve(process.cwd(), "../docs");
const repoRoot = path.resolve(process.cwd(), "..");
const repoSlug = process.env.GITHUB_REPOSITORY ?? "ugoite/ugoite";
const repoBlobBaseUrl = `https://github.com/${repoSlug}/blob/main`;
const repoTreeBaseUrl = `https://github.com/${repoSlug}/tree/main`;
const docsPrefix = "/docs/";
const routeIndexCandidates = [
	"index.md",
	"index.yaml",
	"index.yml",
	"README.md",
];
const routeFileExtensions = [".md", ".yaml", ".yml"];

type HrefParts = {
	pathname: string;
	suffix: string;
};

function splitHref(rawHref: string): HrefParts {
	const queryIndex = rawHref.indexOf("?");
	const hashIndex = rawHref.indexOf("#");
	if (queryIndex === -1 && hashIndex === -1) {
		return { pathname: rawHref, suffix: "" };
	}
	if (queryIndex === -1) {
		return {
			pathname: rawHref.slice(0, hashIndex),
			suffix: rawHref.slice(hashIndex),
		};
	}
	if (hashIndex === -1) {
		return {
			pathname: rawHref.slice(0, queryIndex),
			suffix: rawHref.slice(queryIndex),
		};
	}
	const suffixIndex = Math.min(queryIndex, hashIndex);
	return {
		pathname: rawHref.slice(0, suffixIndex),
		suffix: rawHref.slice(suffixIndex),
	};
}

function stripKnownDocExtension(filePath: string): string {
	return filePath.replace(/\.(md|ya?ml)$/i, "");
}

function fileOrDirectoryType(targetPath: string): "file" | "directory" | null {
	if (!existsSync(targetPath)) {
		return null;
	}
	const stats = statSync(targetPath);
	if (stats.isDirectory()) {
		return "directory";
	}
	if (stats.isFile()) {
		return "file";
	}
	return null;
}

function findDocRouteForTarget(rawDocTarget: string): string | null {
	const normalizedTarget = rawDocTarget.replace(/^\/+/, "").replace(/\\/g, "/");
	const directPath = path.join(docsRoot, normalizedTarget);
	const directType = fileOrDirectoryType(directPath);

	if (directType === "file") {
		return stripKnownDocExtension(normalizedTarget);
	}

	if (directType === "directory") {
		for (const candidate of routeIndexCandidates) {
			const candidatePath = path.join(directPath, candidate);
			if (fileOrDirectoryType(candidatePath) === "file") {
				return stripKnownDocExtension(
					path.posix.join(normalizedTarget, candidate),
				);
			}
		}
		return null;
	}

	for (const extension of routeFileExtensions) {
		const candidatePath = `${directPath}${extension}`;
		if (fileOrDirectoryType(candidatePath) === "file") {
			return stripKnownDocExtension(`${normalizedTarget}${extension}`);
		}
	}

	return null;
}

function githubSourceUrl(repoRelativePath: string): string | null {
	const normalizedPath = repoRelativePath
		.replace(/^\/+/, "")
		.replace(/\\/g, "/");
	if (normalizedPath.length === 0) {
		return `${repoBlobBaseUrl}/README.md`;
	}
	const targetPath = path.join(repoRoot, normalizedPath);
	const targetType = fileOrDirectoryType(targetPath);
	if (targetType === "directory") {
		return `${repoTreeBaseUrl}/${normalizedPath}`;
	}
	if (targetType === "file") {
		return `${repoBlobBaseUrl}/${normalizedPath}`;
	}
	return null;
}

export function resolveDocHref(
	rawHref: string | null | undefined,
	currentDocRelativePath: string,
): string | null {
	if (!rawHref) {
		return null;
	}

	const trimmedHref = rawHref.trim();
	if (trimmedHref.length === 0) {
		return null;
	}

	if (trimmedHref.startsWith("#") || trimmedHref.startsWith("?")) {
		return trimmedHref;
	}

	if (trimmedHref.startsWith("//")) {
		return trimmedHref;
	}

	const schemeMatch = /^([a-zA-Z][a-zA-Z\d+.-]*):/.exec(trimmedHref);
	if (schemeMatch) {
		const scheme = schemeMatch[1].toLowerCase();
		const allowedSchemes = new Set(["http", "https", "mailto"]);
		return allowedSchemes.has(scheme) ? trimmedHref : null;
	}

	const { pathname, suffix } = splitHref(trimmedHref);

	if (pathname.startsWith("/")) {
		if (pathname.startsWith(docsPrefix)) {
			const docsTarget = pathname.slice(docsPrefix.length);
			const docRoute = findDocRouteForTarget(docsTarget);
			if (docRoute) {
				return withBasePath(`/docs/${docRoute}${suffix}`);
			}
			const docsDirectoryUrl = githubSourceUrl(
				path.posix.join("docs", docsTarget),
			);
			if (docsDirectoryUrl) {
				return `${docsDirectoryUrl}${suffix}`;
			}
			return null;
		}

		return withBasePath(`${stripKnownDocExtension(pathname)}${suffix}`);
	}

	const currentRepoDocPath = path.posix.join("/docs", currentDocRelativePath);
	const resolvedRepoPath = path.posix.resolve(
		path.posix.dirname(currentRepoDocPath),
		pathname,
	);

	if (resolvedRepoPath.startsWith("/docs/")) {
		const docsTarget = resolvedRepoPath.slice(docsPrefix.length);
		const docRoute = findDocRouteForTarget(docsTarget);
		if (docRoute) {
			return withBasePath(`/docs/${docRoute}${suffix}`);
		}

		const docsDirectoryUrl = githubSourceUrl(resolvedRepoPath.slice(1));
		if (docsDirectoryUrl) {
			return `${docsDirectoryUrl}${suffix}`;
		}
		return null;
	}

	const repoSourceUrl = githubSourceUrl(resolvedRepoPath.slice(1));
	if (repoSourceUrl) {
		return `${repoSourceUrl}${suffix}`;
	}

	return null;
}
