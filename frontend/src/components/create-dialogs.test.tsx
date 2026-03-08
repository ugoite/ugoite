import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, it, expect, vi } from "vitest";
import { render, fireEvent, screen } from "@solidjs/testing-library";
import { CreateFormDialog, EditFormDialog, CreateEntryDialog } from "./create-dialogs";
import { setLocale } from "~/lib/i18n";
import type { Form } from "~/lib/types";

beforeEach(() => {
	setLocale("en");
});

describe("CreateFormDialog", () => {
	const columnTypes = ["string", "number", "boolean"];

	it("REQ-FE-032: maintains focus on column name input when typing", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<CreateFormDialog
				open={true}
				columnTypes={columnTypes}
				formNames={[]}
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
				formNames={[]}
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

	it("REQ-FE-032: creates form with valid name and fields", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<CreateFormDialog
				open={true}
				columnTypes={columnTypes}
				formNames={[]}
				onClose={onClose}
				onSubmit={onSubmit}
			/>
		));

		fireEvent.input(screen.getByPlaceholderText("e.g. Meeting, Task"), {
			target: { value: "NewForm" },
		});
		fireEvent.click(screen.getByText("+ Add Column"));
		const columnInput = screen.getByPlaceholderText("Column Name") as HTMLInputElement;
		fireEvent.input(columnInput, { target: { value: "field1" } });

		fireEvent.click(screen.getByRole("button", { name: "Create Form" }));

		expect(onSubmit).toHaveBeenCalledWith(
			expect.objectContaining({
				name: "NewForm",
				fields: expect.objectContaining({ field1: expect.any(Object) }),
			}),
		);
	});

	it("REQ-FE-032: removes a column from create form dialog", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<CreateFormDialog
				open={true}
				columnTypes={columnTypes}
				formNames={[]}
				onClose={onClose}
				onSubmit={onSubmit}
			/>
		));

		fireEvent.click(screen.getByText("+ Add Column"));
		expect(screen.getByPlaceholderText("Column Name")).toBeInTheDocument();

		fireEvent.click(screen.getByRole("button", { name: "Remove column" }));
		expect(screen.queryByPlaceholderText("Column Name")).not.toBeInTheDocument();
	});
});

describe("CreateEntryDialog", () => {
	it("REQ-FE-037: requires form selection before creating an entry", async () => {
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

	it("REQ-FE-037: blocks submission when required fields are empty", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Task",
				version: 1,
				fields: { Status: { type: "string", required: true } },
				template: "",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Test Entry" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Task" } });

		const createButton = screen.getByRole("button", { name: "Create" });
		fireEvent.click(createButton);

		expect(onSubmit).not.toHaveBeenCalled();
		expect(screen.getByText("Please fill required fields: Status.")).toBeInTheDocument();
	});

	it("REQ-FE-037: pre-fills defaults for required fields", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Event",
				version: 1,
				fields: {
					Date: { type: "date", required: true },
					Time: { type: "time", required: true },
					StartsAt: { type: "timestamp", required: true },
					Count: { type: "integer", required: true },
				},
				template: "",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Test Entry" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Event" } });

		const dateInput = screen.getByLabelText(/Date/);
		const timeInput = screen.getByLabelText(/Time/);
		const startsAtInput = screen.getByLabelText(/StartsAt/);
		const countInput = screen.getByLabelText(/Count/);

		expect((dateInput as HTMLInputElement).value).not.toBe("");
		expect((timeInput as HTMLInputElement).value).not.toBe("");
		expect((startsAtInput as HTMLInputElement).value).toMatch(/\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/);
		expect((countInput as HTMLInputElement).value).toBe("0");
	});

	it("REQ-FE-037: supports markdown mode submission", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Meeting",
				version: 1,
				fields: { Date: { type: "date", required: true } },
				template: "# Meeting\n\n## Date\n",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "My Markdown Entry" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Meeting" } });
		fireEvent.click(screen.getByRole("button", { name: "Markdown" }));

		const markdownArea = screen.getByRole("textbox", { name: "Markdown input" });
		fireEvent.input(markdownArea, {
			target: { value: "# My Markdown Entry\n\n---\nform: Meeting\n---\n\n## Date\n2026-02-14" },
		});

		fireEvent.click(screen.getByRole("button", { name: "Create" }));

		expect(onSubmit).toHaveBeenCalledWith(
			"My Markdown Entry",
			"Meeting",
			expect.objectContaining({
				__markdown: "# My Markdown Entry\n\n---\nform: Meeting\n---\n\n## Date\n2026-02-14",
			}),
			"markdown",
		);
	});

	it("REQ-FE-053: renders English entry guidance across input modes", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Task",
				version: 1,
				fields: {
					Summary: { type: "string", required: true },
					Done: { type: "boolean", required: false },
					Tags: { type: "list", required: false },
					Project: { type: "row_reference", required: false, target_form: "Project" },
				},
				template: "# Task\n\n## Summary\n",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Localized Task" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Task" } });

		expect(
			screen.getByText("After creation, edit attributes under Markdown `## field name` headings."),
		).toBeInTheDocument();
		expect(
			screen.getByText('Use "- item" or one value per line for list fields.'),
		).toBeInTheDocument();
		expect(
			screen.getByText("Use true/false, yes/no, on/off, or 1/0 for boolean fields."),
		).toBeInTheDocument();
		expect(
			screen.getByText("Enter the target form's entry_id for row_reference fields."),
		).toBeInTheDocument();

		fireEvent.click(screen.getByRole("button", { name: "Markdown" }));
		expect(
			screen.getByText(
				"Markdown content is saved as-is (the backend validates frontmatter/form consistency).",
			),
		).toBeInTheDocument();

		fireEvent.click(screen.getByRole("button", { name: "Chat" }));
		expect(screen.getByText("Chat asks for required fields one at a time.")).toBeInTheDocument();
	});

	it("REQ-FE-037: keeps user-edited markdown when title changes", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Meeting",
				version: 1,
				fields: { Date: { type: "date", required: true } },
				template: "# Meeting\n\n## Date\n",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Meeting" } });
		fireEvent.click(screen.getByRole("button", { name: "Markdown" }));

		const markdownArea = screen.getByRole("textbox", { name: "Markdown input" });
		const customMarkdown = "# Custom\n\n---\nform: Meeting\n---\n\n## Date\n2026-02-14\n";
		fireEvent.input(markdownArea, { target: { value: customMarkdown } });

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Updated title" },
		});

		expect(
			(screen.getByRole("textbox", { name: "Markdown input" }) as HTMLTextAreaElement).value,
		).toBe(customMarkdown);
	});

	it("REQ-FE-037: submits in webform mode successfully", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
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
			target: { value: "My Task" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Task" } });
		fireEvent.click(screen.getByRole("button", { name: "Create" }));

		expect(onSubmit).toHaveBeenCalledWith("My Task", "Task", expect.any(Object), "webform");
	});

	it("REQ-FE-037: shows error when markdown is empty in markdown mode", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Meeting",
				version: 1,
				fields: {},
				template: "",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "My Entry" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Meeting" } });
		fireEvent.click(screen.getByRole("button", { name: "Markdown" }));

		// Set markdown to whitespace only (trims to empty, should show error)
		const markdownArea = screen.getByRole("textbox", { name: "Markdown input" });
		fireEvent.input(markdownArea, { target: { value: "   " } });

		fireEvent.click(screen.getByRole("button", { name: "Create" }));

		expect(onSubmit).not.toHaveBeenCalled();
		expect(screen.getByText("Please provide markdown content.")).toBeInTheDocument();
	});

	it("REQ-FE-037: excludes reserved metadata forms from entry creation", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Assets",
				version: 1,
				fields: {
					link: { type: "string", required: true },
				},
				template: "",
			},
			{
				name: "Meeting",
				version: 1,
				fields: { Date: { type: "date", required: true } },
				template: "",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		const select = screen.getByRole("combobox");
		expect(screen.queryByRole("option", { name: "Assets" })).not.toBeInTheDocument();
		expect(screen.getByRole("option", { name: "Meeting" })).toBeInTheDocument();
		expect((select as HTMLSelectElement).value).toBe("Meeting");
	});
});

describe("EditFormDialog", () => {
	const columnTypes = ["string", "number", "boolean", "row_reference"];
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
				formNames={["ExistingForm"]}
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

	it("REQ-FE-039: shows target form input when row_reference type is selected", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const formWithRowRef: Form = {
			name: "ProjectTask",
			version: 1,
			template: "# ProjectTask\n\n## project\n\n",
			fields: {
				project: { type: "row_reference", required: false, target_form: "Project" },
			},
		};

		render(() => (
			<EditFormDialog
				open={true}
				entryForm={formWithRowRef}
				columnTypes={columnTypes}
				formNames={["ProjectTask", "Project"]}
				onClose={onClose}
				onSubmit={onSubmit}
			/>
		));

		// The row_reference field should show Target Form input
		expect(screen.getByPlaceholderText("e.g. Project")).toBeInTheDocument();
	});

	it("REQ-FE-039: shows default value input for new fields", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<EditFormDialog
				open={true}
				entryForm={mockForm}
				columnTypes={columnTypes}
				formNames={["ExistingForm"]}
				onClose={onClose}
				onSubmit={onSubmit}
			/>
		));

		// Add a new column
		fireEvent.click(screen.getByText("+ Add Column"));

		// New field should have default value input
		expect(screen.getByPlaceholderText("(Optional) e.g. Pending")).toBeInTheDocument();
	});

	it("REQ-FE-032: submits the edited form successfully", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<EditFormDialog
				open={true}
				entryForm={mockForm}
				columnTypes={columnTypes}
				formNames={["ExistingForm"]}
				onClose={onClose}
				onSubmit={onSubmit}
			/>
		));

		fireEvent.click(screen.getByRole("button", { name: "Save Changes" }));

		expect(onSubmit).toHaveBeenCalledWith(
			expect.objectContaining({
				name: "ExistingForm",
				fields: expect.objectContaining({ field1: expect.any(Object) }),
			}),
		);
	});

	it("REQ-FE-032: removes a column from the edit dialog", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<EditFormDialog
				open={true}
				entryForm={mockForm}
				columnTypes={columnTypes}
				formNames={["ExistingForm"]}
				onClose={onClose}
				onSubmit={onSubmit}
			/>
		));

		// Remove the existing field
		const removeButton = screen.getByRole("button", { name: "Remove column" });
		fireEvent.click(removeButton);

		// Submit with no fields
		fireEvent.click(screen.getByRole("button", { name: "Save Changes" }));

		expect(onSubmit).toHaveBeenCalledWith(
			expect.objectContaining({
				name: "ExistingForm",
				fields: {},
			}),
		);
	});

	it("REQ-FE-032: submits new field with default value (processFields)", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<EditFormDialog
				open={true}
				entryForm={mockForm}
				columnTypes={columnTypes}
				formNames={["ExistingForm"]}
				onClose={onClose}
				onSubmit={onSubmit}
			/>
		));

		// Add a new column
		fireEvent.click(screen.getByText("+ Add Column"));

		// Set name for new column
		const inputs = screen.getAllByPlaceholderText("Column Name") as HTMLInputElement[];
		const newInput = inputs.find((i) => i.value === "");
		if (!newInput) throw new Error("new input not found");
		fireEvent.input(newInput, { target: { value: "newcol" } });

		// Set default value
		const defaultInput = screen.getByPlaceholderText("(Optional) e.g. Pending") as HTMLInputElement;
		fireEvent.input(defaultInput, { target: { value: "SomeDefault" } });

		// Submit
		fireEvent.click(screen.getByRole("button", { name: "Save Changes" }));

		expect(onSubmit).toHaveBeenCalledWith(
			expect.objectContaining({
				name: "ExistingForm",
				strategies: expect.objectContaining({ newcol: "SomeDefault" }),
			}),
		);
	});
});
