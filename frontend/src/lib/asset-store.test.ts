// REQ-FE-040: Asset management store
import { describe, it, expect, beforeEach } from "vitest";
import { createRoot } from "solid-js";
import { http, HttpResponse } from "msw";
import { createAssetStore } from "./asset-store";
import { resetMockData, seedSpace } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import type { Space } from "./types";
import { testApiUrl } from "~/test/http-origin";

const testSpace: Space = {
	id: "asset-store-ws",
	name: "Asset Store Space",
	created_at: "2025-01-01T00:00:00Z",
};

describe("createAssetStore", () => {
	beforeEach(() => {
		resetMockData();
		seedSpace(testSpace);
	});

	it("loads assets and updates state", async () => {
		await createRoot(async (dispose) => {
			const store = createAssetStore(() => "asset-store-ws");
			expect(store.assets()).toEqual([]);
			await store.loadAssets();
			expect(store.loading()).toBe(false);
			expect(store.error()).toBeNull();
			dispose();
		});
	});

	it("uploads an asset and reloads", async () => {
		await createRoot(async (dispose) => {
			const store = createAssetStore(() => "asset-store-ws");
			const file = new File(["data"], "test.txt", { type: "text/plain" });
			const asset = await store.uploadAsset(file, "test.txt");
			expect(asset.id).toBeDefined();
			expect(asset.name).toBeDefined();
			dispose();
		});
	});

	it("deletes an asset and reloads", async () => {
		await createRoot(async (dispose) => {
			const store = createAssetStore(() => "asset-store-ws");
			const file = new File(["data"], "delete-me.txt", { type: "text/plain" });
			const asset = await store.uploadAsset(file);
			await store.deleteAsset(asset.id);
			expect(store.error()).toBeNull();
			dispose();
		});
	});

	it("sets error state on load failure", async () => {
		server.use(
			http.get(testApiUrl("/spaces/asset-store-ws/assets"), () =>
				HttpResponse.json({ detail: "Server error" }, { status: 500 }),
			),
		);
		await createRoot(async (dispose) => {
			const store = createAssetStore(() => "asset-store-ws");
			await store.loadAssets();
			expect(store.error()).toContain("Failed to list assets");
			expect(store.loading()).toBe(false);
			dispose();
		});
	});
});
