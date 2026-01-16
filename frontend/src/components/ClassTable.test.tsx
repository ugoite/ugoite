import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, waitFor, fireEvent } from "@solidjs/testing-library";
import { ClassTable } from "./ClassTable";
import { noteApi } from "~/lib/note-api";
import { searchApi } from "~/lib/search-api";

describe("ClassTable", () => {
	it("renders '-' for missing properties and does not throw", async () => {
		const noteClass = {
			name: "Test",
			fields: { A: { type: "string" }, B: { type: "string" } },
		} as any;
		const notes = [
			{ id: "1", title: "Note1", properties: undefined, updated_at: new Date().toISOString() },
		];

		const spy = vi.spyOn(searchApi, "query").mockResolvedValue(notes as any);

		render(() => <ClassTable workspaceId="ws" noteClass={noteClass} onNoteClick={() => {}} />);

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
		const noteClass = {
			name: "Test",
			fields: { price: { type: "number" } },
		} as any;
		const notes = [
			{ id: "1", title: "B Note", properties: { price: 20 }, updated_at: "2026-01-01" },
			{ id: "2", title: "A Note", properties: { price: 10 }, updated_at: "2026-01-02" },
		];

		vi.spyOn(searchApi, "query").mockResolvedValue(notes as any);
		const { getByText } = render(() => (
			<ClassTable workspaceId="ws" noteClass={noteClass} onNoteClick={() => {}} />
		));

		await waitFor(() => expect(getByText("A Note")).toBeInTheDocument());

		// Initially might be in order returned by API. Click Title to sort.
		const titleHeader = getByText("Title");
		fireEvent.click(titleHeader); // Asc null -> asc

		await waitFor(() => {
			const rows = document.querySelectorAll("tbody tr");
			expect(rows[0]).toHaveTextContent("A Note");
		});

		fireEvent.click(titleHeader); // Asc -> desc
		await waitFor(() => {
			const rows = document.querySelectorAll("tbody tr");
			expect(rows[0]).toHaveTextContent("B Note");
		});
	});

	it("REQ-FE-020: filters notes globally", async () => {
		const noteClass = {
			name: "Test",
			fields: { tag: { type: "string" } },
		} as any;
		const notes = [
			{ id: "1", title: "Apple", properties: { tag: "fruit" }, updated_at: "2026-01-01" },
			{ id: "2", title: "Carrot", properties: { tag: "veggie" }, updated_at: "2026-01-01" },
		];

		vi.spyOn(searchApi, "query").mockResolvedValue(notes as any);
		const { getByPlaceholderText, queryByText } = render(() => (
			<ClassTable workspaceId="ws" noteClass={noteClass} onNoteClick={() => {}} />
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
		const noteClass = {
			name: "Test",
			fields: { price: { type: "number" } },
		} as any;
		const notes = [
			{ id: "1", title: "ExportMe", properties: { price: 100 }, updated_at: "2026-01-01" },
		];

		vi.spyOn(searchApi, "query").mockResolvedValue(notes as any);

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
			<ClassTable workspaceId="ws" noteClass={noteClass} onNoteClick={() => {}} />
		));

		await waitFor(() => expect(getByText("Export CSV")).toBeInTheDocument());

		fireEvent.click(getByText("Export CSV"));

		expect(createSpy).toHaveBeenCalled();
		expect(linkClickSpy).toHaveBeenCalled();
	});

	it("REQ-FE-030: Add Row button creates a new note", async () => {
		const noteClass = {
			name: "Test",
			fields: { col: { type: "string" } },
		} as any;
		const notes = [];
		vi.spyOn(searchApi, "query").mockResolvedValue(notes as any);
		const createSpy = vi.spyOn(noteApi, "create").mockResolvedValue({ id: "new-note" } as any);

		const { getByText, getByTitle } = render(() => (
			<ClassTable workspaceId="ws" noteClass={noteClass} onNoteClick={() => {}} />
		));

		// Enable editing first
		const toggleButton = getByTitle("Enable Editing");
		fireEvent.click(toggleButton);

		const addButton = getByText("Add Row");
		fireEvent.click(addButton);

		await waitFor(() => {
			expect(createSpy).toHaveBeenCalledWith(
				"ws",
				expect.objectContaining({
					content: expect.stringContaining("class: Test"),
				}),
			);
		});
		createSpy.mockRestore();
	});

	it("REQ-FE-031: Edit Mode toggle and inline edit", async () => {
		const noteClass = {
			name: "Test",
			fields: { col: { type: "string" } },
		} as any;
		const notes = [
			{ id: "1", title: "Note1", properties: { col: "val" }, updated_at: "2026-01-01" },
		];
		vi.spyOn(searchApi, "query").mockResolvedValue(notes as any);
		const getSpy = vi.spyOn(noteApi, "get").mockResolvedValue({
			id: "1",
			content: "# Note1\n\n## col\nval",
			revision_id: "rev1",
		} as any);
		const updateSpy = vi.spyOn(noteApi, "update").mockResolvedValue({} as any);

		const { getByText, getByTitle, getByDisplayValue } = render(() => (
			<ClassTable workspaceId="ws" noteClass={noteClass} onNoteClick={() => {}} />
		));

		// Wait for render
		await waitFor(() => getByText("Note1"));

		// Click Edit Toggle (Lock icon)
		const toggleButton = getByTitle("Enable Editing");
		fireEvent.click(toggleButton);

		// Now find the cell value and it should be an input or become input on click
		const cell = getByText("val");
		fireEvent.click(cell);

		const input = getByDisplayValue("val");
		fireEvent.input(input, { target: { value: "new-val" } });
		fireEvent.blur(input);

		await waitFor(() => {
			expect(updateSpy).toHaveBeenCalledWith(
				"ws",
				"1",
				expect.objectContaining({
					markdown: expect.stringContaining("new-val"),
					parent_revision_id: "rev1",
				}),
			);
		});
		updateSpy.mockRestore();
		getSpy.mockRestore();
	});

	it("should have a link icon for navigation and not navigate on row click", async () => {
		const noteClass = {
			name: "Test",
			fields: { col: { type: "string" } },
		} as any;
		const notes = [
			{ id: "1", title: "Note1", properties: { col: "val" }, updated_at: "2026-01-01" },
		];
		vi.spyOn(searchApi, "query").mockResolvedValue(notes as any);
		const onNoteClick = vi.fn();

		const { getByText, getByRole } = render(() => (
			<ClassTable workspaceId="ws" noteClass={noteClass} onNoteClick={onNoteClick} />
		));

		await waitFor(() => expect(getByText("Note1")).toBeInTheDocument());

		// Find the row
		const row = getByText("Note1").closest("tr");
		if (!row) throw new Error("Row not found");

		// Click the row itself (but not the link icon)
		fireEvent.click(row);
		expect(onNoteClick).not.toHaveBeenCalled();

		// Find the link icon (title="View Note") and click it
		const linkButton = getByRole("button", { name: /view note/i });
		fireEvent.click(linkButton);
		expect(onNoteClick).toHaveBeenCalledWith("1");
	});

	it("should show restricted lock icon when not in edit mode and open lock icon when in edit mode", async () => {
		const noteClass = { name: "Test", fields: {} } as any;
		vi.spyOn(searchApi, "query").mockResolvedValue([] as any);

		const { getByTitle, queryByTitle } = render(() => (
			<ClassTable workspaceId="ws" noteClass={noteClass} onNoteClick={() => {}} />
		));

		// Initially Locked
		expect(getByTitle("Locked")).toBeInTheDocument();
		expect(queryByTitle("Unlocked")).not.toBeInTheDocument();

		// Toggle to Editable
		const toggleButton = getByTitle("Enable Editing");
		fireEvent.click(toggleButton);

		expect(getByTitle("Unlocked")).toBeInTheDocument();
		expect(queryByTitle("Locked")).not.toBeInTheDocument();
	});
	it("REQ-FE-031: keyboard copy shortcut", async () => {
		const noteClass = {
			name: "Test",
			fields: { col: { type: "string" } },
		} as any;
		const notes = [
			{
				id: "1",
				title: "Note1",
				properties: { col: "val1" },
				updated_at: new Date("2026-01-01").toISOString(),
			},
			{
				id: "2",
				title: "Note2",
				properties: { col: "val2" },
				updated_at: new Date("2026-01-02").toISOString(),
			},
		];
		vi.spyOn(searchApi, "query").mockResolvedValue(notes as any);
		const writeTextSpy = vi.fn().mockResolvedValue(undefined);
		Object.assign(navigator, { clipboard: { writeText: writeTextSpy } });

		const { getByText } = render(() => (
			<ClassTable workspaceId="ws" noteClass={noteClass} onNoteClick={() => {}} />
		));

		await waitFor(() => getByText("Note1"));

		// Simulate drag selection from (0,0) to (1,1)
		// Col 0: Title, Col 1: col
		const cell1 = getByText("Note1");
		const cell2 = getByText("val2");

		fireEvent.mouseDown(cell1);
		fireEvent.mouseEnter(cell2, { buttons: 1 });
		fireEvent.mouseUp(document);

		// Trigger Ctrl+C
		fireEvent.keyDown(document, { key: "c", ctrlKey: true });

		expect(writeTextSpy).toHaveBeenCalledWith("Note1\tval1\nNote2\tval2");
	});

	it("should not trigger custom copy when input is focused", async () => {
		const noteClass = { name: "Test", fields: {} } as any;
		vi.spyOn(searchApi, "query").mockResolvedValue([] as any);
		const writeTextSpy = vi.fn();
		Object.assign(navigator, { clipboard: { writeText: writeTextSpy } });

		const { getByPlaceholderText } = render(() => (
			<ClassTable workspaceId="ws" noteClass={noteClass} onNoteClick={() => {}} />
		));

		const searchInput = getByPlaceholderText("Global Search...");
		searchInput.focus();

		// We need to trigger the keydown event on document since that's where the listener is
		fireEvent.keyDown(document, { key: "c", ctrlKey: true });

		expect(writeTextSpy).not.toHaveBeenCalled();
	});
});
