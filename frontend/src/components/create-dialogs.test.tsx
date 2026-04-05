import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, it, expect, vi } from "vitest";
import { render, fireEvent, screen, waitFor } from "@solidjs/testing-library";
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

	it("REQ-FE-039: hides reserved-name guidance until a reserved name is entered in create form", async () => {
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

		expect(
			screen.queryByText(/Reserved metadata columns are system-owned and cannot be used/),
		).not.toBeInTheDocument();

		fireEvent.input(columnInput, { target: { value: "title" } });
		expect(
			screen.getByText(/Reserved metadata columns are system-owned and cannot be used/),
		).toBeInTheDocument();

		fireEvent.input(columnInput, { target: { value: "summary" } });
		expect(
			screen.queryByText(/Reserved metadata columns are system-owned and cannot be used/),
		).not.toBeInTheDocument();

		fireEvent.input(columnInput, { target: { value: "title" } });
		expect(
			screen.getByText(/Reserved metadata columns are system-owned and cannot be used/),
		).toBeInTheDocument();
	});

	it("REQ-FE-055: keeps create-form column inputs readable on narrow layouts", async () => {
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

		const columnInput = screen.getByPlaceholderText("Column Name");
		expect(columnInput).toHaveClass("w-full");
		expect(columnInput).toHaveClass("sm:min-w-[14rem]");
		expect(columnInput).toHaveClass("sm:flex-1");
		expect(columnInput.parentElement).toHaveClass("flex-col");
		expect(columnInput.parentElement).toHaveClass("sm:flex-row");
		const controls = columnInput.nextElementSibling;
		expect(controls).toHaveClass("grid");
		expect(controls).toHaveClass("grid-cols-[minmax(0,1fr)_auto]");
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

	it("REQ-FE-043: create-form dialog renders rejected submit errors inline", async () => {
		const onSubmit = vi.fn().mockRejectedValue(new Error("Form already exists"));
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
		fireEvent.input(screen.getByPlaceholderText("Column Name"), {
			target: { value: "field1" },
		});

		fireEvent.click(screen.getByRole("button", { name: "Create Form" }));

		expect(await screen.findByText("Form already exists")).toBeInTheDocument();
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

	it("REQ-FE-044: localizes create-form dialog copy in Japanese", async () => {
		setLocale("ja");
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

		expect(screen.getByRole("heading", { name: "新しいフォームを作成" })).toBeInTheDocument();
		expect(screen.getByLabelText("名前")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "+ カラムを追加" })).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "フォームを作成" })).toBeInTheDocument();
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

	it("REQ-FE-043: create-entry dialog renders rejected submit errors inline", async () => {
		const onSubmit = vi.fn().mockRejectedValue(new Error("Entry already exists"));
		const onClose = vi.fn();
		const forms = [
			{
				name: "Task",
				version: 1,
				fields: { Summary: { type: "string", required: false } },
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
		fireEvent.click(screen.getByRole("button", { name: "Create" }));

		expect(await screen.findByText("Entry already exists")).toBeInTheDocument();
	});

	it("REQ-FE-043: create-entry markdown dialog renders rejected submit errors inline", async () => {
		const onSubmit = vi.fn().mockRejectedValue(new Error("Markdown submit failed"));
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
			target: { value: "Markdown Entry" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Meeting" } });
		fireEvent.click(screen.getByRole("button", { name: "Markdown" }));
		fireEvent.input(screen.getByRole("textbox", { name: "Markdown input" }), {
			target: {
				value: "# Markdown Entry\n\n---\nform: Meeting\n---\n\n## Date\n2026-02-14",
			},
		});

		fireEvent.click(screen.getByRole("button", { name: "Create" }));

		expect(await screen.findByText("Markdown submit failed")).toBeInTheDocument();
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

	it("REQ-FE-037: renders optional fields in webform mode", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Task",
				version: 1,
				fields: { Summary: { type: "string", required: false } },
				template: "",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Task with Summary" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Task" } });

		const summaryInput = screen.getByLabelText(/Summary/);
		expect(summaryInput).toBeInTheDocument();
		fireEvent.input(summaryInput, { target: { value: "Optional summary" } });

		fireEvent.click(screen.getByRole("button", { name: "Create" }));

		expect(onSubmit).toHaveBeenCalledWith(
			"Task with Summary",
			"Task",
			expect.objectContaining({ Summary: "Optional summary" }),
			"webform",
		);
	});

	it("REQ-FE-037: sanitizes webform field ids for labels", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Task",
				version: 1,
				fields: { "Due Date / ETA": { type: "string", required: false } },
				template: "",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Task with schedule" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Task" } });

		const scheduleInput = screen.getByLabelText(/Due Date \/ ETA/);
		expect(scheduleInput).toHaveAttribute("id", "webform-0-due-date-eta");
	});

	it("REQ-FE-037: falls back to a stable id when a field name slug is empty", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Task",
				version: 1,
				fields: { "!!!": { type: "string", required: false } },
				template: "",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Task fallback id" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Task" } });

		const fallbackInput = screen.getByRole("textbox", { name: /!!!/ });
		expect(fallbackInput).toHaveAttribute("id", "webform-0-field");
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

	it("REQ-FE-037: clears the markdown flow after successful submission", async () => {
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
			target: { value: "Reset Me" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Meeting" } });
		fireEvent.click(screen.getByRole("button", { name: "Markdown" }));

		const markdownArea = screen.getByRole("textbox", { name: "Markdown input" });
		fireEvent.input(markdownArea, {
			target: { value: "# Reset Me\n\n---\nform: Meeting\n---\n\n## Date\n2026-02-14" },
		});

		const form = screen.getByRole("button", { name: "Create" }).closest("form");
		if (!form) throw new Error("Could not find create-entry form");
		fireEvent.submit(form);

		expect(onSubmit).toHaveBeenCalledWith(
			"Reset Me",
			"Meeting",
			expect.objectContaining({
				__markdown: "# Reset Me\n\n---\nform: Meeting\n---\n\n## Date\n2026-02-14",
			}),
			"markdown",
		);
		await waitFor(() => {
			expect((screen.getByPlaceholderText("Enter entry title...") as HTMLInputElement).value).toBe(
				"",
			);
			expect((screen.getByRole("combobox") as HTMLSelectElement).value).toBe("");
			expect(screen.queryByRole("textbox", { name: "Markdown input" })).not.toBeInTheDocument();
		});
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
			screen.getByText(
				"Fill attributes in the form below. Required fields must be completed before creation.",
			),
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
			screen.getByText("After creation, edit attributes under Markdown `## field name` headings."),
		).toBeInTheDocument();
		expect(
			screen.getByText(
				"Markdown content is saved as-is (the backend validates frontmatter/form consistency).",
			),
		).toBeInTheDocument();

		fireEvent.click(screen.getByRole("button", { name: "Chat" }));
		expect(
			screen.getByText(
				"Chat walks through each field one at a time. Required fields must be answered before creation, and optional fields can be skipped.",
			),
		).toBeInTheDocument();
	});

	it("REQ-FE-053: keeps web-form guidance free of Markdown-only instructions", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Task",
				version: 1,
				fields: {
					Summary: { type: "string", required: true },
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
			screen.getByText(
				"Fill attributes in the form below. Required fields must be completed before creation.",
			),
		).toBeInTheDocument();
		expect(
			screen.queryByText(
				"After creation, edit attributes under Markdown `## field name` headings.",
			),
		).not.toBeInTheDocument();

		fireEvent.click(screen.getByRole("button", { name: "Markdown" }));
		expect(
			screen.getByText("After creation, edit attributes under Markdown `## field name` headings."),
		).toBeInTheDocument();
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
		expect(screen.getByLabelText(/Status/)).toBeInTheDocument();
		fireEvent.click(screen.getByRole("button", { name: "Create" }));

		expect(onSubmit).toHaveBeenCalledWith("My Task", "Task", expect.any(Object), "webform");
	});

	it("REQ-FE-057: chat mode walks through optional fields instead of only required ones", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Task",
				version: 1,
				fields: {
					Summary: { type: "string", required: true },
					Notes: { type: "markdown", required: false },
					Done: { type: "boolean", required: false },
				},
				template: "# Task\n\n## Summary\n",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Chat Task" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Task" } });
		fireEvent.click(screen.getByRole("button", { name: "Chat" }));

		expect(screen.getByText("Question 1 / 3")).toBeInTheDocument();
		expect(screen.getByLabelText(/Summary/)).toBeInTheDocument();

		fireEvent.input(screen.getByLabelText(/Summary/), {
			target: { value: "Conversation summary" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Next question" }));

		expect(screen.getByText("Question 2 / 3")).toBeInTheDocument();
		expect(screen.getByLabelText(/Notes/)).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Skip optional field" })).toBeInTheDocument();
	});
	it("REQ-FE-057: chat step pills react to answers and current step", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Task",
				version: 1,
				fields: {
					Summary: { type: "string", required: true },
					Notes: { type: "markdown", required: false },
				},
				template: "# Task\n\n## Summary\n",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Chat Task" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Task" } });
		fireEvent.click(screen.getByRole("button", { name: "Chat" }));

		expect(screen.getByRole("button", { name: "Summary (required)" })).toHaveClass(
			"ui-button-primary",
		);
		expect(screen.getByRole("button", { name: "Notes (optional)" })).toHaveClass(
			"ui-button-secondary",
		);

		fireEvent.input(screen.getByLabelText(/Summary/), {
			target: { value: "Conversation summary" },
		});

		expect(screen.getByRole("button", { name: "Summary (answered)" })).toHaveClass(
			"ui-button-primary",
		);

		fireEvent.click(screen.getByRole("button", { name: "Next question" }));

		expect(screen.getByRole("button", { name: "Summary (answered)" })).toHaveClass(
			"ui-button-secondary",
		);
		expect(screen.getByRole("button", { name: "Notes (optional)" })).toHaveClass(
			"ui-button-primary",
		);
	});

	it("REQ-FE-057: chat step pills let users jump back to answered questions", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Task",
				version: 1,
				fields: {
					Summary: { type: "string", required: true },
					Notes: { type: "markdown", required: false },
				},
				template: "# Task\n\n## Summary\n",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Chat Task" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Task" } });
		fireEvent.click(screen.getByRole("button", { name: "Chat" }));
		fireEvent.input(screen.getByLabelText(/Summary/), {
			target: { value: "Conversation summary" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Next question" }));
		fireEvent.click(screen.getByRole("button", { name: "Summary (answered)" }));

		expect(screen.getByText("Question 1 / 2")).toBeInTheDocument();
		expect(screen.getByLabelText(/Summary/)).toBeInTheDocument();
	});

	it("REQ-FE-057: chat mode blocks advancing past an empty required field", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Task",
				version: 1,
				fields: {
					Summary: { type: "string", required: true },
					Notes: { type: "markdown", required: false },
				},
				template: "# Task\n\n## Summary\n",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Chat Task" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Task" } });
		fireEvent.click(screen.getByRole("button", { name: "Chat" }));
		fireEvent.click(screen.getByRole("button", { name: "Next question" }));

		expect(screen.getByText("Please answer required field: Summary.")).toBeInTheDocument();
		expect(screen.getByText("Question 1 / 2")).toBeInTheDocument();
		expect(onSubmit).not.toHaveBeenCalled();
	});

	it("REQ-FE-057: chat mode blocks skipping a required field", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Task",
				version: 1,
				fields: {
					Summary: { type: "string", required: true },
					Notes: { type: "markdown", required: false },
				},
				template: "# Task\n\n## Summary\n",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Chat Task" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Task" } });
		fireEvent.click(screen.getByRole("button", { name: "Chat" }));
		fireEvent.click(screen.getByRole("button", { name: "Skip field" }));

		expect(screen.getByText("Required field cannot be skipped: Summary.")).toBeInTheDocument();
		expect(screen.getByText("Question 1 / 2")).toBeInTheDocument();
		expect(onSubmit).not.toHaveBeenCalled();
	});
	it("REQ-FE-057: chat mode submits answered required and optional fields", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Task",
				version: 1,
				fields: {
					Summary: { type: "string", required: true },
					Notes: { type: "markdown", required: false },
				},
				template: "# Task\n\n## Summary\n",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Chat Task" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Task" } });
		fireEvent.click(screen.getByRole("button", { name: "Chat" }));

		fireEvent.input(screen.getByLabelText(/Summary/), {
			target: { value: "Conversation summary" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Next question" }));
		fireEvent.input(screen.getByLabelText(/Notes/), { target: { value: "Optional note" } });
		fireEvent.click(screen.getByRole("button", { name: "Create" }));

		expect(onSubmit).toHaveBeenCalledWith(
			"Chat Task",
			"Task",
			expect.objectContaining({
				Summary: "Conversation summary",
				Notes: "Optional note",
			}),
			"chat",
		);
	});

	it("REQ-FE-057: skipping an optional chat field clears its draft answer", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
			{
				name: "Task",
				version: 1,
				fields: {
					Summary: { type: "string", required: true },
					Notes: { type: "markdown", required: false },
				},
				template: "# Task\n\n## Summary\n",
			},
		];

		render(() => (
			<CreateEntryDialog open={true} forms={forms} onClose={onClose} onSubmit={onSubmit} />
		));

		fireEvent.input(screen.getByPlaceholderText("Enter entry title..."), {
			target: { value: "Chat Task" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Task" } });
		fireEvent.click(screen.getByRole("button", { name: "Chat" }));
		fireEvent.input(screen.getByLabelText(/Summary/), {
			target: { value: "Conversation summary" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Next question" }));
		fireEvent.input(screen.getByLabelText(/Notes/), { target: { value: "Draft note" } });
		fireEvent.click(screen.getByRole("button", { name: "Skip optional field" }));
		fireEvent.click(screen.getByRole("button", { name: "Create" }));

		expect(onSubmit).toHaveBeenCalledWith(
			"Chat Task",
			"Task",
			expect.not.objectContaining({
				Notes: "Draft note",
			}),
			"chat",
		);
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

	it("REQ-FE-037: keeps the form open and shows required-field errors on submit", async () => {
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
			target: { value: "Needs Status" },
		});
		fireEvent.change(screen.getByRole("combobox"), { target: { value: "Task" } });

		const form = screen.getByRole("button", { name: "Create" }).closest("form");
		if (!form) throw new Error("Could not find create-entry form");
		fireEvent.submit(form);

		expect(onSubmit).not.toHaveBeenCalled();
		expect(screen.getByText("Please fill required fields: Status.")).toBeInTheDocument();
		expect((screen.getByPlaceholderText("Enter entry title...") as HTMLInputElement).value).toBe(
			"Needs Status",
		);
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

	it("REQ-FE-037: closes the create-entry dialog from the backdrop and Escape key", async () => {
		const onSubmit = vi.fn();
		const onClose = vi.fn();

		render(() => (
			<CreateEntryDialog open={true} forms={[]} onClose={onClose} onSubmit={onSubmit} />
		));

		const dialog = screen.getByRole("dialog");
		fireEvent.keyDown(dialog, { key: "Escape" });
		onClose.mockClear();
		fireEvent.click(dialog, { target: dialog });

		expect(onClose).toHaveBeenCalled();
	});

	it("REQ-FE-044: localizes create-entry dialog copy in Japanese", async () => {
		setLocale("ja");
		const onSubmit = vi.fn();
		const onClose = vi.fn();
		const forms = [
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

		expect(screen.getByRole("heading", { name: "新しいエントリを作成" })).toBeInTheDocument();
		expect(screen.getByLabelText("タイトル")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Webフォーム" })).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "作成" })).toBeInTheDocument();
		expect(screen.getByText("フォームフィールド")).toBeInTheDocument();
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

	it("REQ-FE-055: keeps edit-form column inputs readable on narrow layouts", async () => {
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

		fireEvent.click(screen.getByText("+ Add Column"));

		const inputs = screen.getAllByPlaceholderText("Column Name") as HTMLInputElement[];
		const newInput = inputs.find((input) => input.value === "");
		if (!newInput) throw new Error("Could not find new edit-dialog column input");

		expect(newInput).toHaveClass("w-full");
		expect(newInput).toHaveClass("sm:min-w-[14rem]");
		expect(newInput).toHaveClass("sm:flex-1");
		expect(newInput.parentElement).toHaveClass("flex-col");
		expect(newInput.parentElement).toHaveClass("sm:flex-row");
		const controls = newInput.nextElementSibling;
		expect(controls).toHaveClass("grid");
		expect(controls).toHaveClass("grid-cols-[minmax(0,1fr)_auto]");
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

	it("REQ-FE-039: hides reserved-name guidance until a reserved name is entered in edit form", async () => {
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

		fireEvent.click(screen.getByText("+ Add Column"));

		const inputs = screen.getAllByPlaceholderText("Column Name") as HTMLInputElement[];
		const newInput = inputs.find((input) => input.value === "");
		if (!newInput) throw new Error("Could not find new edit-dialog column input");

		expect(
			screen.queryByText(/Reserved metadata columns are system-owned and cannot be used/),
		).not.toBeInTheDocument();

		fireEvent.input(newInput, { target: { value: "title" } });
		expect(
			screen.getByText(/Reserved metadata columns are system-owned and cannot be used/),
		).toBeInTheDocument();

		fireEvent.input(newInput, { target: { value: "summary" } });
		expect(
			screen.queryByText(/Reserved metadata columns are system-owned and cannot be used/),
		).not.toBeInTheDocument();

		fireEvent.input(newInput, { target: { value: "title" } });
		expect(
			screen.getByText(/Reserved metadata columns are system-owned and cannot be used/),
		).toBeInTheDocument();
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

	it("REQ-FE-044: localizes rejected edit-form submit fallback in Japanese", async () => {
		setLocale("ja");
		const onSubmit = vi.fn().mockRejectedValue("network");
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

		fireEvent.click(screen.getByRole("button", { name: "変更を保存" }));

		expect(await screen.findByText("フォームの更新に失敗しました")).toBeInTheDocument();
	});
});
