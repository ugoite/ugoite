// REQ-FE-004: Note list display
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { ListPanel } from "./ListPanel";
import type { Class, NoteRecord } from "~/lib/types";

describe("ListPanel", () => {
	const mockClasses: Class[] = [
		{
			name: "Meeting",
			fields: { date: { type: "date" }, attendees: { type: "string" } },
			template: "",
		},
		{ name: "Task", fields: { status: { type: "string" } }, template: "" },
	];

	const mockNotes: NoteRecord[] = [
		{
			id: "note-1",
			title: "Test Note 1",
			class: "Meeting",
			updated_at: "2026-01-01T00:00:00Z",
			created_at: "2026-01-01T00:00:00Z",
			properties: { date: "2026-01-01" },
			links: [],
		},
		{
			id: "note-2",
			title: "Test Note 2",
			class: null,
			updated_at: "2026-01-02T00:00:00Z",
			created_at: "2026-01-02T00:00:00Z",
			properties: {},
			links: [],
		},
	];

	describe("Notes mode", () => {
		it("should render New Note button", () => {
			const [filterClass, setFilterClass] = createSignal("");
			render(() => (
				<ListPanel
					mode="notes"
					classes={mockClasses}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
					onCreate={vi.fn()}
				/>
			));
			expect(screen.getByText("New Note")).toBeInTheDocument();
		});

		it("should call onCreate when create button is clicked", () => {
			const [filterClass, setFilterClass] = createSignal("");
			const onCreate = vi.fn();
			render(() => (
				<ListPanel
					mode="notes"
					classes={mockClasses}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
					onCreate={onCreate}
				/>
			));
			fireEvent.click(screen.getByText("New Note"));
			expect(onCreate).toHaveBeenCalled();
		});

		it("should render class filter dropdown", () => {
			const [filterClass, setFilterClass] = createSignal("");
			render(() => (
				<ListPanel
					mode="notes"
					classes={mockClasses}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
				/>
			));
			expect(screen.getByText("Filter by Class")).toBeInTheDocument();
			expect(screen.getByRole("combobox")).toBeInTheDocument();
		});

		it("should call onFilterClassChange when filter changes", () => {
			const [filterClass, _setFilterClass] = createSignal("");
			const onFilterClassChange = vi.fn();
			render(() => (
				<ListPanel
					mode="notes"
					classes={mockClasses}
					filterClass={filterClass}
					onFilterClassChange={onFilterClassChange}
				/>
			));
			const select = screen.getByRole("combobox");
			fireEvent.change(select, { target: { value: "Meeting" } });
			expect(onFilterClassChange).toHaveBeenCalledWith("Meeting");
		});

		it("should render notes list", () => {
			const [filterClass, setFilterClass] = createSignal("");
			render(() => (
				<ListPanel
					mode="notes"
					classes={mockClasses}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
					notes={mockNotes}
				/>
			));
			expect(screen.getByText("Test Note 1")).toBeInTheDocument();
			expect(screen.getByText("Test Note 2")).toBeInTheDocument();
		});

		it("should highlight selected note", () => {
			const [filterClass, setFilterClass] = createSignal("");
			render(() => (
				<ListPanel
					mode="notes"
					classes={mockClasses}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
					notes={mockNotes}
					selectedId="note-1"
				/>
			));
			const selectedButton = screen.getByText("Test Note 1").closest("button");
			expect(selectedButton).toHaveClass("selected");
		});

		it("should call onSelectNote when a note is clicked", () => {
			const [filterClass, setFilterClass] = createSignal("");
			const onSelectNote = vi.fn();
			render(() => (
				<ListPanel
					mode="notes"
					classes={mockClasses}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
					notes={mockNotes}
					onSelectNote={onSelectNote}
				/>
			));
			fireEvent.click(screen.getByText("Test Note 1"));
			expect(onSelectNote).toHaveBeenCalledWith("note-1");
		});

		it("should show loading state", () => {
			const [filterClass, setFilterClass] = createSignal("");
			render(() => (
				<ListPanel
					mode="notes"
					classes={mockClasses}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
					loading={true}
				/>
			));
			expect(screen.getByText("Loading notes...")).toBeInTheDocument();
		});

		it("should show error state", () => {
			const [filterClass, setFilterClass] = createSignal("");
			render(() => (
				<ListPanel
					mode="notes"
					classes={mockClasses}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
					error="Test error message"
				/>
			));
			expect(screen.getByText("Test error message")).toBeInTheDocument();
		});

		it("should show empty state when no notes", () => {
			const [filterClass, setFilterClass] = createSignal("");
			render(() => (
				<ListPanel
					mode="notes"
					classes={mockClasses}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
					notes={[]}
				/>
			));
			expect(screen.getByText("No notes yet")).toBeInTheDocument();
		});
	});

	describe("Classes mode", () => {
		it("should render New Class button", () => {
			const [filterClass, setFilterClass] = createSignal("");
			render(() => (
				<ListPanel
					mode="classes"
					classes={mockClasses}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
					onCreate={vi.fn()}
				/>
			));
			expect(screen.getByText("New Class")).toBeInTheDocument();
		});

		it("should render classes list", () => {
			const [filterClass, setFilterClass] = createSignal("");
			render(() => (
				<ListPanel
					mode="classes"
					classes={mockClasses}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
				/>
			));
			expect(screen.queryByText("Filter by Class")).not.toBeInTheDocument();
			// "Meeting" appears both in filter dropdown and class list
			expect(screen.getAllByText("Meeting").length).toBeGreaterThanOrEqual(1);
			expect(screen.getAllByText("Task").length).toBeGreaterThanOrEqual(1);
			// Check for "fields" text which only appears in class list
			expect(screen.getByText(/2\s+fields/)).toBeInTheDocument();
			expect(screen.getByText(/1\s+field/)).toBeInTheDocument();
		});

		it("should highlight selected class", () => {
			const [filterClass, setFilterClass] = createSignal("");
			render(() => (
				<ListPanel
					mode="classes"
					classes={mockClasses}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
					selectedClass={mockClasses[0]}
				/>
			));
			// Find the button containing "2 fields" which is in the Meeting class item
			const fieldsText = screen.getByText("2 fields");
			const selectedButton = fieldsText.closest("button");
			expect(selectedButton).toHaveClass("ring-2");
		});

		it("should call onSelectClass when a class is clicked", () => {
			const [filterClass, setFilterClass] = createSignal("");
			const onSelectClass = vi.fn();
			render(() => (
				<ListPanel
					mode="classes"
					classes={mockClasses}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
					onSelectClass={onSelectClass}
				/>
			));
			// Click on the class button (not dropdown) by clicking on "2 fields"
			const fieldsText = screen.getByText("2 fields");
			const button = fieldsText.closest("button");
			if (button) {
				fireEvent.click(button);
			}
			expect(onSelectClass).toHaveBeenCalledWith(mockClasses[0]);
		});

		it("should show empty state when no classes", () => {
			const [filterClass, setFilterClass] = createSignal("");
			render(() => (
				<ListPanel
					mode="classes"
					classes={[]}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
				/>
			));
			expect(screen.getByText("No classes yet")).toBeInTheDocument();
		});
	});
});
