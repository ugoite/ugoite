// REQ-FE-004: Entry list display
// REQ-FE-008: Entry selection and highlight
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { EntryList } from "./EntryList";
import { resetMockData, seedSpace, seedEntry } from "~/test/mocks/handlers";
import type { Entry, EntryRecord, Space } from "~/lib/types";

const testSpace: Space = {
	id: "ui-test-ws",
	name: "UI Test Space",
	created_at: "2025-01-01T00:00:00Z",
};

// Helper to create controlled props
const createControlledProps = (initialEntries: EntryRecord[] = []) => {
	const [entries, setEntries] = createSignal(initialEntries);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);
	return { entries, setEntries, loading, setLoading, error, setError };
};

describe("EntryList", () => {
	beforeEach(() => {
		resetMockData();
		seedSpace(testSpace);
	});

	describe("controlled mode", () => {
		it("should render empty state when no entries exist", async () => {
			const { entries, loading, error } = createControlledProps();
			render(() => <EntryList entries={entries} loading={loading} error={error} />);

			expect(screen.getByText(/no entries/i)).toBeInTheDocument();
		});

		it("should render list of entries with titles", async () => {
			const record1: EntryRecord = {
				id: "entry-1",
				title: "First Entry",
				updated_at: "2025-01-01T00:00:00Z",
				properties: {},
				tags: [],
				links: [],
			};
			const record2: EntryRecord = {
				id: "entry-2",
				title: "Second Entry",
				updated_at: "2025-01-02T00:00:00Z",
				properties: { Status: "Active" },
				tags: [],
				links: [],
			};

			const { entries, loading, error } = createControlledProps([record1, record2]);
			render(() => <EntryList entries={entries} loading={loading} error={error} />);

			expect(screen.getByText("First Entry")).toBeInTheDocument();
			expect(screen.getByText("Second Entry")).toBeInTheDocument();
		});

		it("should display extracted properties in entry cards", async () => {
			const record: EntryRecord = {
				id: "prop-entry",
				title: "Meeting",
				updated_at: "2025-01-01T00:00:00Z",
				properties: { Date: "2025-01-15", Status: "Completed" },
				tags: [],
				links: [],
			};

			const { entries, loading, error } = createControlledProps([record]);
			render(() => <EntryList entries={entries} loading={loading} error={error} />);

			expect(screen.getByText("Meeting")).toBeInTheDocument();
			// Check for the property key (Date:) and value (2025-01-15)
			expect(screen.getByText("Date:")).toBeInTheDocument();
			expect(screen.getByText("2025-01-15")).toBeInTheDocument();
		});

		it("should handle entries with null or undefined properties gracefully", async () => {
			// Simulate API response where properties may be null/undefined
			const recordWithNullProperties = {
				id: "null-prop-entry",
				title: "Entry without properties",
				updated_at: "2025-01-01T00:00:00Z",
				properties: null as unknown as Record<string, unknown>,
				tags: [],
				links: [],
			} as EntryRecord;

			const recordWithUndefinedProperties = {
				id: "undefined-prop-entry",
				title: "Entry with undefined properties",
				updated_at: "2025-01-02T00:00:00Z",
				tags: [],
				links: [],
			} as EntryRecord;

			const { entries, loading, error } = createControlledProps([
				recordWithNullProperties,
				recordWithUndefinedProperties,
			]);

			// Should not throw an error
			render(() => <EntryList entries={entries} loading={loading} error={error} />);

			expect(screen.getByText("Entry without properties")).toBeInTheDocument();
			expect(screen.getByText("Entry with undefined properties")).toBeInTheDocument();
		});

		it("should call onSelect when a entry is clicked", async () => {
			const record: EntryRecord = {
				id: "click-entry",
				title: "Clickable Entry",
				updated_at: "2025-01-01T00:00:00Z",
				properties: {},
				tags: [],
				links: [],
			};

			const { entries, loading, error } = createControlledProps([record]);
			const onSelect = vi.fn();
			render(() => (
				<EntryList entries={entries} loading={loading} error={error} onSelect={onSelect} />
			));

			fireEvent.click(screen.getByText("Clickable Entry"));

			expect(onSelect).toHaveBeenCalledWith("click-entry");
		});

		it("should show loading state", () => {
			const { entries, loading, setLoading, error } = createControlledProps();
			setLoading(true);
			render(() => <EntryList entries={entries} loading={loading} error={error} />);

			expect(screen.getByText(/loading/i)).toBeInTheDocument();
		});

		it("should highlight selected entry", async () => {
			const record: EntryRecord = {
				id: "selected-entry",
				title: "Selected Entry",
				updated_at: "2025-01-01T00:00:00Z",
				properties: {},
				tags: [],
				links: [],
			};

			const { entries, loading, error } = createControlledProps([record]);
			render(() => (
				<EntryList
					entries={entries}
					loading={loading}
					error={error}
					selectedEntryId="selected-entry"
				/>
			));

			const button = screen.getByRole("button");
			expect(button).toHaveClass("ui-card-selected");
		});
	});

	describe("standalone mode", () => {
		it("should render empty state when no entries exist", async () => {
			render(() => <EntryList spaceId="ui-test-ws" />);

			await waitFor(() => {
				expect(screen.getByText(/no entries/i)).toBeInTheDocument();
			});
		});

		it("should render list of entries with titles", async () => {
			const entry1: Entry = {
				id: "entry-1",
				content: "# First Entry",
				revision_id: "rev-1",
				created_at: "2025-01-01T00:00:00Z",
				updated_at: "2025-01-01T00:00:00Z",
			};
			const record1: EntryRecord = {
				id: "entry-1",
				title: "First Entry",
				updated_at: "2025-01-01T00:00:00Z",
				properties: {},
				tags: [],
				links: [],
			};
			const entry2: Entry = {
				id: "entry-2",
				content: "# Second Entry",
				revision_id: "rev-2",
				created_at: "2025-01-02T00:00:00Z",
				updated_at: "2025-01-02T00:00:00Z",
			};
			const record2: EntryRecord = {
				id: "entry-2",
				title: "Second Entry",
				updated_at: "2025-01-02T00:00:00Z",
				properties: { Status: "Active" },
				tags: [],
				links: [],
			};

			seedEntry("ui-test-ws", entry1, record1);
			seedEntry("ui-test-ws", entry2, record2);

			render(() => <EntryList spaceId="ui-test-ws" />);

			await waitFor(() => {
				expect(screen.getByText("First Entry")).toBeInTheDocument();
				expect(screen.getByText("Second Entry")).toBeInTheDocument();
			});
		});

		it("should call onSelect when a entry is clicked", async () => {
			const entry: Entry = {
				id: "click-entry",
				content: "# Clickable Entry",
				revision_id: "rev-click",
				created_at: "2025-01-01T00:00:00Z",
				updated_at: "2025-01-01T00:00:00Z",
			};
			const record: EntryRecord = {
				id: "click-entry",
				title: "Clickable Entry",
				updated_at: "2025-01-01T00:00:00Z",
				properties: {},
				tags: [],
				links: [],
			};

			seedEntry("ui-test-ws", entry, record);

			const onSelect = vi.fn();
			render(() => <EntryList spaceId="ui-test-ws" onSelect={onSelect} />);

			await waitFor(() => {
				expect(screen.getByText("Clickable Entry")).toBeInTheDocument();
			});

			fireEvent.click(screen.getByText("Clickable Entry"));

			expect(onSelect).toHaveBeenCalledWith("click-entry");
		});
	});
});
