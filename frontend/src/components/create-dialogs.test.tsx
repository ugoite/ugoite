import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, fireEvent, screen } from "@solidjs/testing-library";
import { CreateSchemaDialog } from "./create-dialogs";

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
