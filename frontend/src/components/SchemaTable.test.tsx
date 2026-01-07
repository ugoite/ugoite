import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, waitFor, fireEvent } from "@solidjs/testing-library";
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

	it("REQ-FE-019: sorts notes when clicking headers", async () => {
		const schema = {
			name: "Test",
			fields: { price: { type: "number" } },
		} as any;
		const notes = [
			{ id: "1", title: "B Note", properties: { price: 20 }, updated_at: "2026-01-01" },
			{ id: "2", title: "A Note", properties: { price: 10 }, updated_at: "2026-01-02" },
		];

		vi.spyOn(workspaceApi, "query").mockResolvedValue(notes as any);
		const { getByText, getAllByRole } = render(() => (
			<SchemaTable workspaceId="ws" schema={schema} onNoteClick={() => {}} />
		));

		await waitFor(() => expect(getByText("A Note")).toBeInTheDocument());

		// Initially might be in order returned by API. Click Title to sort.
		const titleHeader = getByText("Title");
		fireEvent.click(titleHeader); // Asc null -> asc

		await waitFor(() => {
			const rows = getAllByRole("button").filter((n) => n.tagName === "TR");
			expect(rows[0]).toHaveTextContent("A Note");
		});

		fireEvent.click(titleHeader); // Asc -> desc
		await waitFor(() => {
			const rows = getAllByRole("button").filter((n) => n.tagName === "TR");
			expect(rows[0]).toHaveTextContent("B Note");
		});
	});

	it("REQ-FE-020: filters notes globally", async () => {
		const schema = {
			name: "Test",
			fields: { tag: { type: "string" } },
		} as any;
		const notes = [
			{ id: "1", title: "Apple", properties: { tag: "fruit" }, updated_at: "2026-01-01" },
			{ id: "2", title: "Carrot", properties: { tag: "veggie" }, updated_at: "2026-01-01" },
		];

		vi.spyOn(workspaceApi, "query").mockResolvedValue(notes as any);
		const { getByPlaceholderText, queryByText } = render(() => (
			<SchemaTable workspaceId="ws" schema={schema} onNoteClick={() => {}} />
		));

		await waitFor(() => expect(queryByText("Apple")).toBeInTheDocument());

		const searchInput = getByPlaceholderText("Global Search...");
		fireEvent.input(searchInput, { target: { value: "carrot" } });

		await waitFor(() => {
			expect(queryByText("Carrot")).toBeInTheDocument();
			expect(queryByText("Apple")).not.toBeInTheDocument();
		});
	});

	it("REQ-FE-021: exports filtered data to CSV", async () => {
		const schema = {
			name: "Test",
			fields: { price: { type: "number" } },
		} as any;
		const notes = [
			{ id: "1", title: "ExportMe", properties: { price: 100 }, updated_at: "2026-01-01" },
		];

		vi.spyOn(workspaceApi, "query").mockResolvedValue(notes as any);

		// Mock URL.createObjectURL/revokeObjectURL
		const createSpy = vi.fn().mockReturnValue("blob:test");
		const revokeSpy = vi.fn();
		global.URL.createObjectURL = createSpy;
		global.URL.revokeObjectURL = revokeSpy;

		// Mock anchor element
		const linkClickSpy = vi.fn();
		const originalCreateElement = document.createElement;
		vi.spyOn(document, "createElement").mockImplementation((tag) => {
			const el = originalCreateElement.call(document, tag);
			if (tag === "a") {
				(el as any).click = linkClickSpy;
			}
			return el;
		});

		const { getByText } = render(() => (
			<SchemaTable workspaceId="ws" schema={schema} onNoteClick={() => {}} />
		));

		await waitFor(() => expect(getByText("Export CSV")).toBeInTheDocument());

		fireEvent.click(getByText("Export CSV"));

		expect(createSpy).toHaveBeenCalled();
		expect(linkClickSpy).toHaveBeenCalled();
	});
});
