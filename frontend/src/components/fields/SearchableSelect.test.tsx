import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { SearchableSelect } from "./SearchableSelect";

describe("SearchableSelect", () => {
  it("REQ-UX-SEARCHABLE-001: renders human titles without option IDs", () => {
    const options = [
      { id: "entry-id-opaque", title: "Meeting notes" },
      { id: "entry-id-next", title: "Project plan" },
    ];
    const onSelect = vi.fn();
    const { container } = render(() => (
      <SearchableSelect
        id="entry-search"
        query=""
        onQueryInput={() => {}}
        options={options}
        selected={options[0]}
        onSelect={onSelect}
        onClear={() => {}}
        loading={false}
        error={null}
        labels={{
          placeholder: "Search entries",
          help: "Type to search",
          selectedHeading: "Selected",
          clear: "Clear",
          loading: "Loading",
          loadError: "Could not load",
          noMatches: "No matches",
        }}
      />
    ));

    expect(container).toHaveTextContent("Meeting notes");
    expect(container).toHaveTextContent("Project plan");
    expect(container).not.toHaveTextContent("entry-id-opaque");
    expect(container).not.toHaveTextContent("entry-id-next");
    expect(screen.getByRole("option", { name: "Meeting notes" }))
      .toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Project plan" }));
    expect(onSelect).toHaveBeenCalledWith(options[1]);
  });
});
