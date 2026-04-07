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

	it("REQ-FE-060: summarizes file-backed spaces as saved local metadata", () => {
		expect(
			summarizeSpaceStorage({
				...baseSpace,
				storage_config: { uri: "file:///var/lib/ugoite/demo" },
			}),
		).toEqual({
			label: "Saved local filesystem URI",
			description:
				"This space metadata includes a local filesystem URI. Current writes still use the storage configured by this backend deployment.",
			uri: "file:///var/lib/ugoite/demo",
		});
	});

	it("REQ-FE-060: summarizes s3-backed spaces as saved object-store metadata", () => {
		expect(
			summarizeSpaceStorage({
				...baseSpace,
				storage_config: { uri: "s3://ugoite-prod/spaces/demo" },
			}),
		).toEqual({
			label: "Saved object-store URI",
			description:
				"This space metadata includes an object-store URI. Current writes still use the storage configured by this backend deployment.",
			uri: "s3://ugoite-prod/spaces/demo",
		});
	});

	it("REQ-FE-060: falls back to backend-managed storage when no URI is present", () => {
		expect(summarizeSpaceStorage(baseSpace)).toEqual({
			label: "Backend-managed storage",
			description:
				"No per-space storage URI is saved, so current writes use the storage configured by this backend deployment.",
			uri: null,
		});
	});

	it("REQ-FE-060: ignores blank storage URIs when summarizing saved metadata", () => {
		expect(
			summarizeSpaceStorage({
				...baseSpace,
				storage_config: { uri: "   " },
			}),
		).toEqual({
			label: "Backend-managed storage",
			description:
				"No per-space storage URI is saved, so current writes use the storage configured by this backend deployment.",
			uri: null,
		});
	});

	it("REQ-FE-060: treats unrecognized storage URIs as backend-managed metadata", () => {
		expect(
			summarizeSpaceStorage({
				...baseSpace,
				storage_config: { uri: "https://backend.example.internal/spaces/demo" },
			}),
		).toEqual({
			label: "Backend-managed storage",
			description:
				"No per-space storage URI is saved, so current writes use the storage configured by this backend deployment.",
			uri: "https://backend.example.internal/spaces/demo",
		});
	});
});
