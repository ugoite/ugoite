import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, fireEvent, screen } from "@solidjs/testing-library";
import { CreateFormDialog, EditFormDialog, CreateEntryDialog } from "./create-dialogs";
import type { Form } from "~/lib/types";

describe("CreateFormDialog", () => {
	const columnTypes = ["string", "number", "boolean"];

	it("REQ-FE-032: maintains focus on column name input when typing", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<CreateFormDialog
				open={true}
				columnTypes={columnTypes}
				onClose={onClose}
				onSubmit={onSubmit}
			/>
		));

		// Enter entryForm name
		const nameInput = screen.getByPlaceholderText("e.g. Meeting, Task");
		fireEvent.input(nameInput, { target: { value: "TestForm" } });

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
			<CreateFormDialog
				open={true}
				columnTypes={columnTypes}
				onClose={onClose}
				onSubmit={onSubmit}
			/>
		));

		fireEvent.input(screen.getByPlaceholderText("e.g. Meeting, Task"), {
			target: { value: "TestForm" },
		});
		fireEvent.click(screen.getByText("+ Add Column"));
		const columnInput = screen.getByPlaceholderText("Column Name") as HTMLInputElement;
		fireEvent.input(columnInput, { target: { value: "title" } });

		expect(screen.getByText("Reserved metadata column name")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Create Form" })).toBeDisabled();
	});
});

describe("CreateEntryDialog", () => {
	it("REQ-FE-037: requires form selection before creating a entry", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
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
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Test Entry" },
		});

		const createButton = screen.getByRole("button", { name: "Create" });
		expect(createButton).toBeDisabled();

		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Meeting" } });
		expect(createButton).not.toBeDisabled();
	});
});

describe("EditFormDialog", () => {
	const columnTypes = ["string", "number", "boolean"];
	const mockForm: Form = {
		name: "ExistingForm",
		version: 1,
		template: "# ExistingForm\n\n## field1\n\n",
		fields: {
			field1: { type: "string", required: false },
		},
	};

	it("REQ-FE-032: maintains focus on column name input when typing in edit dialog", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<EditFormDialog
				open={true}
				entryForm={mockForm}
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
