// REQ-FE-020: FormList component
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";
import { FormList } from "./FormList";
import type { Form } from "~/lib/types";

const forms: Form[] = [
	{ name: "Meeting", fields: { Date: { type: "date" } } },
	{ name: "Task", fields: { Status: { type: "text" } } },
];

describe("FormList", () => {
	it("renders form names", () => {
		const onSelect = vi.fn();
		render(() => <FormList entryForms={forms} selectedForm={null} onSelect={onSelect} />);
		expect(screen.getByText("Meeting")).toBeInTheDocument();
		expect(screen.getByText("Task")).toBeInTheDocument();
	});

	it("shows field count for each form", () => {
		const onSelect = vi.fn();
		render(() => <FormList entryForms={forms} selectedForm={null} onSelect={onSelect} />);
		expect(screen.getAllByText("1 fields")).toHaveLength(2);
	});

	it("calls onSelect when a form is clicked", () => {
		const onSelect = vi.fn();
		render(() => <FormList entryForms={forms} selectedForm={null} onSelect={onSelect} />);
		fireEvent.click(screen.getByText("Meeting"));
		expect(onSelect).toHaveBeenCalledWith(forms[0]);
	});

	it("applies selected styling when form is selected", () => {
		const onSelect = vi.fn();
		render(() => <FormList entryForms={forms} selectedForm={forms[0]} onSelect={onSelect} />);
		const buttons = screen.getAllByRole("button");
		expect(buttons[0]).toHaveClass("ui-list-item-selected");
		expect(buttons[1]).not.toHaveClass("ui-list-item-selected");
	});

	it("renders empty list with no forms", () => {
		const onSelect = vi.fn();
		render(() => <FormList entryForms={[]} selectedForm={null} onSelect={onSelect} />);
		expect(screen.queryByRole("button")).not.toBeInTheDocument();
	});
});
