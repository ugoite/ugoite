import tailwind from "@astrojs/tailwind";
import { defineConfig } from "astro/config";

const repoName = process.env.GITHUB_REPOSITORY?.split("/")[1] ?? "";
const inferredBase =
	process.env.GITHUB_ACTIONS === "true" && repoName ? `/${repoName}` : "/";
const configuredBase = process.env.DOCSITE_BASE ?? inferredBase;
const withLeadingSlash = configuredBase.startsWith("/")
	? configuredBase
	: `/${configuredBase}`;
const base = withLeadingSlash.endsWith("/")
	? withLeadingSlash
	: `${withLeadingSlash}/`;

export default defineConfig({
	integrations: [tailwind()],
	site: process.env.DOCSITE_ORIGIN ?? "http://localhost:4321",
	base,
	vite: {
		server: {
			fs: {
				allow: ["../docs", "../shared", "."],
			},
		},
	},
});
