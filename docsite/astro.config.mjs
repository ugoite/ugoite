import tailwind from "@astrojs/tailwind";
import { defineConfig } from "astro/config";

export default defineConfig({
	integrations: [tailwind()],
	site: process.env.DOCSITE_ORIGIN ?? "http://localhost:4321",
	vite: {
		server: {
			fs: {
				allow: ["../docs", "../shared"],
			},
		},
	},
});
