import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, fireEvent, screen } from "@solidjs/testing-library";
import { CreateSchemaDialog, EditSchemaDialog } from "./create-dialogs";
import type { Schema } from "~/lib/types";

describe("CreateSchemaDialog", () => {
	const columnTypes = ["string", "number", "boolean"];

	it("REQ-FE-032: maintains focus on column name input when typing", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<CreateSchemaDialog
				open={true}
				columnTypes={columnTypes}
				onClose={onClose}
				onSubmit={onSubmit}
			/>
		));

		// Enter schema name
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
});

describe("EditSchemaDialog", () => {
	const columnTypes = ["string", "number", "boolean"];
	const mockSchema: Schema = {
		name: "ExistingClass",
		template: "# ExistingClass\n\n## field1\n\n",
		fields: {
			field1: { type: "string", required: false },
		},
	};

	it("REQ-FE-032: maintains focus on column name input when typing in edit dialog", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<EditSchemaDialog
				open={true}
				schema={mockSchema}
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
