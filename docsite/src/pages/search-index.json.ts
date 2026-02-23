import { promises as fs } from "node:fs";
import path from "node:path";
import type { APIRoute } from "astro";
import { withBasePath } from "../lib/base-path";
import { getNavSectionsWithChildren, topLinks } from "../lib/navigation";

type SearchItem = {
	title: string;
	href: string;
	content: string;
};

function titleFromPath(relativePath: string): string {
	const base = relativePath
		.replace(/\.(md|ya?ml)$/i, "")
		.split("/")
		.at(-1) ?? relativePath;
	return base.replaceAll(/[-_]/g, " ").replace(/\b\w/g, (char) => char.toUpperCase());
}

function toPlainText(raw: string): string {
	return raw
		.replaceAll(/```[\s\S]*?```/g, " ")
		.replaceAll(/`[^`]*`/g, " ")
		.replaceAll(/!\[[^\]]*]\([^)]*\)/g, " ")
		.replaceAll(/\[[^\]]*]\([^)]*\)/g, " ")
		.replaceAll(/[#>*_~\-]+/g, " ")
		.replaceAll(/\s+/g, " ")
		.trim();
}

async function readDocItems(): Promise<SearchItem[]> {
	const docsRoot = path.resolve(process.cwd(), "../docs");
	const walk = async (dir: string): Promise<string[]> => {
		const entries = await fs.readdir(dir, { withFileTypes: true });
		const files = await Promise.all(
			entries.map(async (entry) => {
				const fullPath = path.join(dir, entry.name);
				if (entry.isDirectory()) return await walk(fullPath);
				if (entry.isFile() && /\.(md|ya?ml)$/i.test(entry.name)) return [fullPath];
				return [];
			}),
		);
		return files.flat();
	};

	const docFiles = await walk(docsRoot);
	const items = await Promise.all(
		docFiles.map(async (fullPath) => {
			const relative = path.relative(docsRoot, fullPath).replaceAll("\\", "/");
			const source = await fs.readFile(fullPath, "utf-8");
			return {
				title: titleFromPath(relative),
				href: withBasePath(`/docs/${relative.replace(/\.(md|ya?ml)$/i, "")}`),
				content: toPlainText(source).slice(0, 6000),
			};
		}),
	);
	return items;
}

export const GET: APIRoute = async () => {
	const sections = await getNavSectionsWithChildren();
	const navItems: SearchItem[] = [
		...topLinks.map((item) => ({
			title: item.title,
			href: withBasePath(item.href),
			content: "",
		})),
		...sections.flatMap((section) => [
			...section.items.map((item) => ({
				title: item.title,
				href: withBasePath(item.href),
				content: "",
			})),
			...section.items.flatMap((item) =>
				(item.items ?? []).map((child) => ({
					title: child.title,
					href: withBasePath(child.href),
					content: "",
				})),
			),
		]),
	];
	const docItems = await readDocItems();
	const deduped = new Map<string, SearchItem>();
	for (const item of [...navItems, ...docItems]) {
		deduped.set(`${item.href}:${item.title}`, item);
	}

	return new Response(JSON.stringify([...deduped.values()]), {
		headers: {
			"content-type": "application/json; charset=utf-8",
			"cache-control": "public, max-age=300",
		},
	});
};
