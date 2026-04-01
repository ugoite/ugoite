import { beforeEach, describe, expect, it } from "vitest";
import { setLocale } from "~/lib/i18n";
import { summarizeSpaceStorage } from "./storage-topology";
import type { Space } from "~/lib/types";

const baseSpace: Space = {
	id: "demo",
	name: "Demo Space",
	created_at: "2025-01-01T00:00:00Z",
};

describe("summarizeSpaceStorage", () => {
	beforeEach(() => {
		setLocale("en");
	});

	it("REQ-FE-060: summarizes file-backed spaces as local filesystem", () => {
		expect(
			summarizeSpaceStorage({
				...baseSpace,
				storage_config: { uri: "file:///var/lib/ugoite/demo" },
			}),
		).toEqual({
			label: "Local filesystem",
			description: "This space writes directly to a local path on this machine.",
			uri: "file:///var/lib/ugoite/demo",
		});
	});

	it("REQ-FE-060: summarizes s3-backed spaces as remote object store", () => {
		expect(
			summarizeSpaceStorage({
				...baseSpace,
				storage_config: { uri: "s3://ugoite-prod/spaces/demo" },
			}),
		).toEqual({
			label: "Remote object store",
			description: "This space writes to object storage through its configured connector.",
			uri: "s3://ugoite-prod/spaces/demo",
		});
	});

	it("REQ-FE-060: summarizes documented object-store schemes as remote object store", () => {
		const uris = [
			"gcs://ugoite-prod/spaces/demo",
			"gs://ugoite-prod/spaces/demo",
			"azblob://ugoite-prod/spaces/demo",
			"azdls://ugoite-prod/spaces/demo",
			"abfs://ugoite-prod/spaces/demo",
			"abfss://ugoite-prod/spaces/demo",
			"oss://ugoite-prod/spaces/demo",
		];

		for (const uri of uris) {
			expect(
				summarizeSpaceStorage({
					...baseSpace,
					storage_config: { uri },
				}),
			).toEqual({
				label: "Remote object store",
				description: "This space writes to object storage through its configured connector.",
				uri,
			});
		}
	});

	it("REQ-FE-060: falls back to backend API when no storage URI is present", () => {
		expect(summarizeSpaceStorage(baseSpace)).toEqual({
			label: "Backend API",
			description: "This space uses the storage configured by this backend deployment.",
			uri: null,
		});
	});

	it("REQ-FE-060: ignores blank storage URIs when summarizing topology", () => {
		expect(
			summarizeSpaceStorage({
				...baseSpace,
				storage_config: { uri: "   " },
			}),
		).toEqual({
			label: "Backend API",
			description: "This space uses the storage configured by this backend deployment.",
			uri: null,
		});
	});

	it("REQ-FE-060: treats unrecognized storage URIs as backend-managed topology", () => {
		expect(
			summarizeSpaceStorage({
				...baseSpace,
				storage_config: { uri: "https://backend.example.internal/spaces/demo" },
			}),
		).toEqual({
			label: "Backend API",
			description: "This space uses the storage configured by this backend deployment.",
			uri: "https://backend.example.internal/spaces/demo",
		});
	});
});
