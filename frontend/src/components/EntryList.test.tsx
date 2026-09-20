// REQ-FE-004: Entry list display
// REQ-FE-008: Entry selection and highlight
import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { EntryList } from "./EntryList";
import { resetMockData, seedEntry, seedSpace } from "~/test/mocks/handlers";
import type { Entry, EntryRecord, Space } from "~/lib/types";

const testSpace: Space = {
  space_uid: "ui-test-ws",
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
      render(() => (
        <EntryList entries={entries} loading={loading} error={error} />
      ));

      expect(screen.getByText(/no entries/i)).toBeInTheDocument();
    });

    it("should render list of entries with IDs", async () => {
      const record1: EntryRecord = {
        id: "entry-1",
        updated_at: "2025-01-01T00:00:00Z",
        properties: {},
        tags: [],
      };
      const record2: EntryRecord = {
        id: "entry-2",
        updated_at: "2025-01-02T00:00:00Z",
        properties: { Status: "Active" },
        tags: [],
      };

      const { entries, loading, error } = createControlledProps([
        record1,
        record2,
      ]);
      render(() => (
        <EntryList entries={entries} loading={loading} error={error} />
      ));

      expect(screen.getByText("entry-1")).toBeInTheDocument();
      expect(screen.getByText("entry-2")).toBeInTheDocument();
    });

    it("should display extracted properties in entry cards", async () => {
      const record: EntryRecord = {
        id: "prop-entry",
        updated_at: "2025-01-01T00:00:00Z",
        properties: { Date: "2025-01-15", Status: "Completed" },
        tags: [],
      };

      const { entries, loading, error } = createControlledProps([record]);
      render(() => (
        <EntryList entries={entries} loading={loading} error={error} />
      ));

      expect(screen.getByText("prop-entry")).toBeInTheDocument();
      // Check for the property key (Date:) and value (2025-01-15)
      expect(screen.getByText("Date:")).toBeInTheDocument();
      expect(screen.getByText("2025-01-15")).toBeInTheDocument();
    });

    it("should handle entries with null or undefined properties gracefully", async () => {
      // Simulate API response where properties may be null/undefined
      const recordWithNullProperties = {
        id: "null-prop-entry",
        updated_at: "2025-01-01T00:00:00Z",
        properties: null as unknown as Record<string, unknown>,
        tags: [],
      } as EntryRecord;

      const recordWithUndefinedProperties = {
        id: "undefined-prop-entry",
        updated_at: "2025-01-02T00:00:00Z",
        tags: [],
      } as EntryRecord;

      const { entries, loading, error } = createControlledProps([
        recordWithNullProperties,
        recordWithUndefinedProperties,
      ]);

      // Should not throw an error
      render(() => (
        <EntryList entries={entries} loading={loading} error={error} />
      ));

      expect(screen.getByText("null-prop-entry")).toBeInTheDocument();
      expect(screen.getByText("undefined-prop-entry"))
        .toBeInTheDocument();
    });

    it("should call onSelect when a entry is clicked", async () => {
      const record: EntryRecord = {
        id: "click-entry",
        updated_at: "2025-01-01T00:00:00Z",
        properties: {},
        tags: [],
      };

      const { entries, loading, error } = createControlledProps([record]);
      const onSelect = vi.fn();
      render(() => (
        <EntryList
          entries={entries}
          loading={loading}
          error={error}
          onSelect={onSelect}
        />
      ));

      fireEvent.click(screen.getByText("click-entry"));

      expect(onSelect).toHaveBeenCalledWith("click-entry");
    });

    it("should show loading state", () => {
      const { entries, loading, setLoading, error } = createControlledProps();
      setLoading(true);
      const { container } = render(() => (
        <EntryList entries={entries} loading={loading} error={error} />
      ));

      // Spinner-only indicator: label is sr-only for assistive technology.
      const status = screen.getByRole("status");
      expect(status).toHaveTextContent(/loading/i);
      expect(container.querySelector(".localspinner")).toBeInTheDocument();
      expect(container.querySelector(".ui-sr-only")).toHaveTextContent(
        /loading/i,
      );
      expect(container.querySelector(".entry-list-container")).toHaveAttribute(
        "aria-busy",
        "true",
      );
    });

    it("should keep existing rows mounted while loading", () => {
      const record: EntryRecord = {
        id: "kept-entry",
        updated_at: "2025-01-01T00:00:00Z",
        properties: {},
        tags: [],
      };
      const { entries, loading, setLoading, error } = createControlledProps([
        record,
      ]);
      setLoading(true);
      render(() => (
        <EntryList entries={entries} loading={loading} error={error} />
      ));

      expect(screen.getByText("kept-entry")).toBeInTheDocument();
      expect(screen.getByRole("status")).toBeInTheDocument();
    });

    it("should highlight selected entry", async () => {
      const record: EntryRecord = {
        id: "selected-entry",
        updated_at: "2025-01-01T00:00:00Z",
        properties: {},
        tags: [],
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

    it("should display form badge and non-string properties", () => {
      const record: EntryRecord = {
        id: "form-entry",
        form: "Meeting",
        updated_at: "2025-01-01T00:00:00Z",
        properties: {
          Count: 5,
          Active: true,
          Structured: { nested: "value" },
        },
        tags: [],
      };
      const { entries, loading, error } = createControlledProps([record]);
      render(() => (
        <EntryList entries={entries} loading={loading} error={error} />
      ));
      expect(screen.getByText("form-entry")).toBeInTheDocument();
      expect(screen.getByText("Meeting")).toBeInTheDocument();
      expect(screen.getByText("5")).toBeInTheDocument();
      expect(screen.queryByText("[object Object]")).not.toBeInTheDocument();
    });
  });

  describe("standalone mode", () => {
    it("should render empty state when no entries exist", async () => {
      render(() => <EntryList spaceId="ui-test-ws" />);

      await waitFor(() => {
        expect(screen.getByText(/no entries/i)).toBeInTheDocument();
      });
    });

    it("should render list of entries with IDs", async () => {
      const entry1: Entry = {
        id: "entry-1",
        content: "# First Entry",
        revision_id: "rev-1",
        created_at: "2025-01-01T00:00:00Z",
        updated_at: "2025-01-01T00:00:00Z",
      };
      const record1: EntryRecord = {
        id: "entry-1",
        updated_at: "2025-01-01T00:00:00Z",
        properties: {},
        tags: [],
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
        updated_at: "2025-01-02T00:00:00Z",
        properties: { Status: "Active" },
        tags: [],
      };

      seedEntry("ui-test-ws", entry1, record1);
      seedEntry("ui-test-ws", entry2, record2);

      render(() => <EntryList spaceId="ui-test-ws" />);

      await waitFor(() => {
        expect(screen.getByText("entry-1")).toBeInTheDocument();
        expect(screen.getByText("entry-2")).toBeInTheDocument();
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
        updated_at: "2025-01-01T00:00:00Z",
        properties: {},
        tags: [],
      };

      seedEntry("ui-test-ws", entry, record);

      const onSelect = vi.fn();
      render(() => <EntryList spaceId="ui-test-ws" onSelect={onSelect} />);

      await waitFor(() => {
        expect(screen.getByText("click-entry")).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText("click-entry"));

      expect(onSelect).toHaveBeenCalledWith("click-entry");
    });
  });
});
