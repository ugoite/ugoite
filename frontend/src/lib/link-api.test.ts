// REQ-FE-010: Link API (removed stub)
import { describe, it, expect } from "vitest";
import { linkApi } from "./link-api";

describe("linkApi", () => {
	it("create throws links removed error", async () => {
		await expect(linkApi.create("ws", { source: "a", target: "b", kind: "ref" })).rejects.toThrow(
			"Links API has been removed",
		);
	});

	it("list throws links removed error", async () => {
		await expect(linkApi.list("ws")).rejects.toThrow("Links API has been removed");
	});

	it("delete throws links removed error", async () => {
		await expect(linkApi.delete("ws", "link-1")).rejects.toThrow("Links API has been removed");
	});
});
