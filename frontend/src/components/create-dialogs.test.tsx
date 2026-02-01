import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, fireEvent, screen } from "@solidjs/testing-library";
import { CreateClassDialog, EditClassDialog, CreateNoteDialog } from "./create-dialogs";
import type { Class } from "~/lib/types";

describe("CreateClassDialog", () => {
	const columnTypes = ["string", "number", "boolean"];

	it("REQ-FE-032: maintains focus on column name input when typing", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<CreateClassDialog
				open={true}
				columnTypes={columnTypes}
				onClose={onClose}
				onSubmit={onSubmit}
			/>
		));

		// Enter noteClass name
		const nameInput = screen.getByPlaceholderText("e.g. Meeting, Task");
		fireEvent.input(nameInput, { target: { value: "TestClass" } });

		// Add a column
		const addButton = screen.getByText("+ Add Column");
		fireEvent.click(addButton);

		// Find the column name input
		const columnInput = screen.getByPlaceholderText("Column Name") as HTMLInputElement;
		columnInput.focus();
		expect(document.activeElement).toBe(columnInput);

		// Type the first character
		fireEvent.input(columnInput, { target: { value: "f" } });

		// Check if focus is STILL on the input
		// In the buggy version using <For>, this should FAIL because the input is recreated
		expect(document.activeElement).toBe(columnInput);

		// Type the second character
		fireEvent.input(columnInput, { target: { value: "fi" } });
		expect(document.activeElement).toBe(columnInput);
	});

	it("REQ-FE-039: blocks reserved metadata column names", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<CreateClassDialog
				open={true}
				columnTypes={columnTypes}
				onClose={onClose}
				onSubmit={onSubmit}
			/>
		));

		fireEvent.input(screen.getByPlaceholderText("e.g. Meeting, Task"), {
			target: { value: "TestClass" },
		});
		fireEvent.click(screen.getByText("+ Add Column"));
		const columnInput = screen.getByPlaceholderText("Column Name") as HTMLInputElement;
		fireEvent.input(columnInput, { target: { value: "title" } });

		expect(screen.getByText("Reserved metadata column name")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Create Class" })).toBeDisabled();
	});
});

describe("CreateNoteDialog", () => {
	it("REQ-FE-037: requires class selection before creating a note", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const classes = [
			{
				name: "Meeting",
				version: 1,
				fields: { Date: { type: "date", required: true } },
				template: "",
			},
			{
				name: "Task",
				version: 1,
				fields: { Status: { type: "string", required: false } },
				template: "",
			},
		];

		render(() => (
			<CreateNoteDialog open={true} classes={classes} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter note title..."), {
			target: { value: "Test Note" },
		});

		const createButton = screen.getByRole("button", { name: "Create" });
		expect(createButton).toBeDisabled();

		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Meeting" } });
		expect(createButton).not.toBeDisabled();
	});
});

describe("EditClassDialog", () => {
	const columnTypes = ["string", "number", "boolean"];
	const mockClass: Class = {
		name: "ExistingClass",
		version: 1,
		template: "# ExistingClass\n\n## field1\n\n",
		fields: {
			field1: { type: "string", required: false },
		},
	};

	it("REQ-FE-032: maintains focus on column name input when typing in edit dialog", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<EditClassDialog
				open={true}
				noteClass={mockClass}
				columnTypes={columnTypes}
				onClose={onClose}
				onSubmit={onSubmit}
			/>
		));

		// Add a new column
		const addButton = screen.getByText("+ Add Column");
		fireEvent.click(addButton);

		// Find the new column name input (the one that is empty)
		const inputs = screen.getAllByPlaceholderText("Column Name") as HTMLInputElement[];
		const columnInput = inputs.find((i) => i.value === "");
		if (!columnInput) throw new Error("Could not find new column input");

		columnInput.focus();
		expect(document.activeElement).toBe(columnInput);

		// Type the first character
		fireEvent.input(columnInput, { target: { value: "g" } });

		// Check if focus is STILL on the input
		expect(document.activeElement).toBe(columnInput);

		// Type the second character
		fireEvent.input(columnInput, { target: { value: "ge" } });
		expect(document.activeElement).toBe(columnInput);
	});
});
