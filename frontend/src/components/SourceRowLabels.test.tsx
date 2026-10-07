import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { setLocale } from "~/lib/i18n";
import { formatDateLabel } from "~/lib/date-format";
import {
  FormRowLabel,
  SavedSqlRowLabel,
  savedSqlRowMeta,
  savedSqlRowSecondary,
} from "./SourceRowLabels";

const plainEntry = {
  name: "Monthly",
  kind: "user-query" as const,
  metadata: null,
  variables: [],
  updated_at: "2026-10-02T00:00:00Z",
};

describe("SourceRowLabels", () => {
  beforeEach(() => {
    setLocale("en");
  });

  afterEach(() => cleanup());

  it("renders a user form with a glyph and no system marker", () => {
    render(() => <FormRowLabel name="Tasks" />);
    expect(screen.getByText("T")).toHaveClass("glyph");
    expect(screen.getByText("Tasks")).toHaveClass("formRowName");
    expect(screen.queryByLabelText("System form")).toBeNull();
  });

  it("marks reserved metadata forms with the system marker", () => {
    render(() => <FormRowLabel name="SQL" />);
    expect(screen.getByText("SQL")).toHaveClass("formRowName");
    expect(screen.getByLabelText("System form")).toBeInTheDocument();
  });

  it("renders saved sql names with variables and updated meta", () => {
    const entry = {
      ...plainEntry,
      variables: [
        { type: "string", name: "title", description: "Title" },
      ],
    };
    render(() => (
      <>
        <SavedSqlRowLabel entry={entry} />
        <span data-testid="secondary">{savedSqlRowSecondary(entry)}</span>
        <span data-testid="meta">{savedSqlRowMeta(entry)}</span>
      </>
    ));
    expect(screen.getByText("Monthly")).toBeInTheDocument();
    expect(screen.getByTestId("secondary")).toHaveTextContent("Variables");
    expect(screen.getByTestId("meta")).toHaveTextContent(
      formatDateLabel(entry.updated_at),
    );
    expect(savedSqlRowSecondary(plainEntry)).toBeUndefined();
  });
});
