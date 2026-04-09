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

	it("REQ-FE-060: summarizes backend local storage as local filesystem", () => {
		expect(
			summarizeSpaceStorage({
				...baseSpace,
				storage: { type: "local", root: "/var/lib/ugoite/demo" },
			}),
		).toEqual({
			label: "Local filesystem",
			description:
				"This space currently writes through the backend deployment's local filesystem root.",
			uri: "file:///var/lib/ugoite/demo",
		});
	});

	it("REQ-FE-060: preserves already formatted backend storage URIs", () => {
		expect(
			summarizeSpaceStorage({
				...baseSpace,
				storage: { type: "local", root: "file:///var/lib/ugoite/demo" },
			}),
		).toEqual({
			label: "Local filesystem",
			description:
				"This space currently writes through the backend deployment's local filesystem root.",
			uri: "file:///var/lib/ugoite/demo",
		});
	});

	it("REQ-FE-060: preserves relative local storage roots verbatim", () => {
		expect(
			summarizeSpaceStorage({
				...baseSpace,
				storage: { type: "fs", root: "./relative/demo" },
			}),
		).toEqual({
			label: "Local filesystem",
			description:
				"This space currently writes through the backend deployment's local filesystem root.",
			uri: "./relative/demo",
		});
	});

	it("REQ-FE-060: summarizes backend s3 storage as remote object store", () => {
		expect(
			summarizeSpaceStorage({
				...baseSpace,
				storage: { type: "s3", root: "ugoite-prod/spaces/demo" },
			}),
		).toEqual({
			label: "Remote object store",
			description:
				"This space currently writes through the backend deployment's object storage root.",
			uri: "s3://ugoite-prod/spaces/demo",
		});
	});

	it("REQ-FE-060: falls back to backend API when no backend storage metadata is present", () => {
		expect(summarizeSpaceStorage(baseSpace)).toEqual({
			label: "Backend API",
			description: "This space currently uses the storage configured by this backend deployment.",
			uri: null,
		});
	});

	it("REQ-FE-060: ignores blank backend storage roots when summarizing topology", () => {
		expect(
			summarizeSpaceStorage({
				...baseSpace,
				storage: { type: "local", root: "   " },
			}),
		).toEqual({
			label: "Backend API",
			description: "This space currently uses the storage configured by this backend deployment.",
			uri: null,
		});
	});

	it("REQ-FE-060: treats unrecognized backend storage types as backend-managed topology", () => {
		expect(
			summarizeSpaceStorage({
				...baseSpace,
				storage: { type: "webdav", root: "files.example.internal/spaces/demo" },
			}),
		).toEqual({
			label: "Backend API",
			description: "This space currently uses the storage configured by this backend deployment.",
			uri: "webdav://files.example.internal/spaces/demo",
		});
	});
});
