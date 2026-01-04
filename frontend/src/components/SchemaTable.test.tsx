import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, waitFor } from "@solidjs/testing-library";
import { SchemaTable } from "./SchemaTable";
import { workspaceApi } from "~/lib/client";

describe("SchemaTable", () => {
	it("renders '-' for missing properties and does not throw", async () => {
		const schema = {
			name: "Test",
			fields: { A: { type: "string" }, B: { type: "string" } },
		} as any;
		const notes = [
			{ id: "1", title: "Note1", properties: undefined, updated_at: new Date().toISOString() },
		];

		const spy = vi.spyOn(workspaceApi, "query").mockResolvedValue(notes as any);

		render(() => <SchemaTable workspaceId="ws" schema={schema} onNoteClick={() => {}} />);

		await waitFor(() => {
			expect(spy).toHaveBeenCalled();
			const matches = document.querySelectorAll("td");
			// Ensure at least one cell contains the placeholder
			const hyphens = Array.from(matches).filter((n) => n.textContent === "-");
			expect(hyphens.length).toBeGreaterThan(0);
		});

		spy.mockRestore();
	});
});
