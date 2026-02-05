// REQ-FE-004: Entry list display
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { ListPanel } from "./ListPanel";
import type { Form, EntryRecord } from "~/lib/types";

describe("ListPanel", () => {
	const mockForms: Form[] = [
		{
			name: "Meeting",
			version: 1,
			fields: { date: { type: "date" }, attendees: { type: "string" } },
			template: "",
		},
		{ name: "Task", version: 1, fields: { status: { type: "string" } }, template: "" },
	];

	const mockEntries: EntryRecord[] = [
		{
			id: "entry-1",
			title: "Test Entry 1",
			form: "Meeting",
			updated_at: "2026-01-01T00:00:00Z",
			created_at: "2026-01-01T00:00:00Z",
			properties: { date: "2026-01-01" },
			links: [],
		},
		{
			id: "entry-2",
			title: "Test Entry 2",
			form: null,
			updated_at: "2026-01-02T00:00:00Z",
			created_at: "2026-01-02T00:00:00Z",
			properties: {},
			links: [],
		},
	];

	describe("Entries mode", () => {
		it("should render New Entry button", () => {
			const [filterForm, setFilterForm] = createSignal("");
			render(() => (
				<ListPanel
					mode="entries"
					forms={mockForms}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
					onCreate={vi.fn()}
				/>
			));
			expect(screen.getByText("New Entry")).toBeInTheDocument();
		});

		it("should call onCreate when create button is clicked", () => {
			const [filterForm, setFilterForm] = createSignal("");
			const onCreate = vi.fn();
			render(() => (
				<ListPanel
					mode="entries"
					forms={mockForms}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
					onCreate={onCreate}
				/>
			));
			fireEvent.click(screen.getByText("New Entry"));
			expect(onCreate).toHaveBeenCalled();
		});

		it("REQ-FE-037: disables new entry when no forms exist", () => {
			const [filterForm, setFilterForm] = createSignal("");
			render(() => (
				<ListPanel
					mode="entries"
					forms={[]}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
					createDisabled={true}
					createDisabledReason="Create a form before adding entries."
				/>
			));
			const button = screen.getByRole("button", { name: "New Entry" });
			expect(button).toBeDisabled();
			expect(screen.getByText("Create a form before adding entries.")).toBeInTheDocument();
		});

		it("should render form filter dropdown", () => {
			const [filterForm, setFilterForm] = createSignal("");
			render(() => (
				<ListPanel
					mode="entries"
					forms={mockForms}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
				/>
			));
			expect(screen.getByText("Filter by Form")).toBeInTheDocument();
			expect(screen.getByRole("combobox")).toBeInTheDocument();
		});

		it("should call onFilterFormChange when filter changes", () => {
			const [filterForm, _setFilterForm] = createSignal("");
			const onFilterFormChange = vi.fn();
			render(() => (
				<ListPanel
					mode="entries"
					forms={mockForms}
					filterForm={filterForm}
					onFilterFormChange={onFilterFormChange}
				/>
			));
			const select = screen.getByRole("combobox");
			fireEvent.change(select, { target: { value: "Meeting" } });
			expect(onFilterFormChange).toHaveBeenCalledWith("Meeting");
		});

		it("should render entries list", () => {
			const [filterForm, setFilterForm] = createSignal("");
			render(() => (
				<ListPanel
					mode="entries"
					forms={mockForms}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
					entries={mockEntries}
				/>
			));
			expect(screen.getByText("Test Entry 1")).toBeInTheDocument();
			expect(screen.getByText("Test Entry 2")).toBeInTheDocument();
		});

		it("should highlight selected entry", () => {
			const [filterForm, setFilterForm] = createSignal("");
			render(() => (
				<ListPanel
					mode="entries"
					forms={mockForms}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
					entries={mockEntries}
					selectedId="entry-1"
				/>
			));
			const selectedButton = screen.getByText("Test Entry 1").closest("button");
			expect(selectedButton).toHaveClass("selected");
		});

		it("should call onSelectEntry when a entry is clicked", () => {
			const [filterForm, setFilterForm] = createSignal("");
			const onSelectEntry = vi.fn();
			render(() => (
				<ListPanel
					mode="entries"
					forms={mockForms}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
					entries={mockEntries}
					onSelectEntry={onSelectEntry}
				/>
			));
			fireEvent.click(screen.getByText("Test Entry 1"));
			expect(onSelectEntry).toHaveBeenCalledWith("entry-1");
		});

		it("should show loading state", () => {
			const [filterForm, setFilterForm] = createSignal("");
			render(() => (
				<ListPanel
					mode="entries"
					forms={mockForms}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
					loading={true}
				/>
			));
			expect(screen.getByText("Loading entries...")).toBeInTheDocument();
		});

		it("should show error state", () => {
			const [filterForm, setFilterForm] = createSignal("");
			render(() => (
				<ListPanel
					mode="entries"
					forms={mockForms}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
					error="Test error message"
				/>
			));
			expect(screen.getByText("Test error message")).toBeInTheDocument();
		});

		it("should show empty state when no entries", () => {
			const [filterForm, setFilterForm] = createSignal("");
			render(() => (
				<ListPanel
					mode="entries"
					forms={mockForms}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
					entries={[]}
				/>
			));
			expect(screen.getByText("No entries yet")).toBeInTheDocument();
		});
	});

	describe("Forms mode", () => {
		it("should render New Form button", () => {
			const [filterForm, setFilterForm] = createSignal("");
			render(() => (
				<ListPanel
					mode="forms"
					forms={mockForms}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
					onCreate={vi.fn()}
				/>
			));
			expect(screen.getByText("New Form")).toBeInTheDocument();
		});

		it("should render forms list", () => {
			const [filterForm, setFilterForm] = createSignal("");
			render(() => (
				<ListPanel
					mode="forms"
					forms={mockForms}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
				/>
			));
			expect(screen.queryByText("Filter by Form")).not.toBeInTheDocument();
			// "Meeting" appears both in filter dropdown and form list
			expect(screen.getAllByText("Meeting").length).toBeGreaterThanOrEqual(1);
			expect(screen.getAllByText("Task").length).toBeGreaterThanOrEqual(1);
			// Check for "fields" text which only appears in form list
			expect(screen.getByText(/2\s+fields/)).toBeInTheDocument();
			expect(screen.getByText(/1\s+field/)).toBeInTheDocument();
		});

		it("should highlight selected form", () => {
			const [filterForm, setFilterForm] = createSignal("");
			render(() => (
				<ListPanel
					mode="forms"
					forms={mockForms}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
					selectedForm={mockForms[0]}
				/>
			));
			// Find the button containing "2 fields" which is in the Meeting form item
			const fieldsText = screen.getByText("2 fields");
			const selectedButton = fieldsText.closest("button");
			expect(selectedButton).toHaveClass("ring-2");
		});

		it("should call onSelectForm when a form is clicked", () => {
			const [filterForm, setFilterForm] = createSignal("");
			const onSelectForm = vi.fn();
			render(() => (
				<ListPanel
					mode="forms"
					forms={mockForms}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
					onSelectForm={onSelectForm}
				/>
			));
			// Click on the form button (not dropdown) by clicking on "2 fields"
			const fieldsText = screen.getByText("2 fields");
			const button = fieldsText.closest("button");
			if (button) {
				fireEvent.click(button);
			}
			expect(onSelectForm).toHaveBeenCalledWith(mockForms[0]);
		});

		it("should show empty state when no forms", () => {
			const [filterForm, setFilterForm] = createSignal("");
			render(() => (
				<ListPanel
					mode="forms"
					forms={[]}
					filterForm={filterForm}
					onFilterFormChange={setFilterForm}
				/>
			));
			expect(screen.getByText("No forms yet")).toBeInTheDocument();
		});
	});
});
