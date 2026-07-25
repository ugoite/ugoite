import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { setLocale } from "~/lib/i18n";
import SpaceEntryDetailRoute from "./index";

const navigate = vi.fn();
const refetchForms = vi.fn();

vi.mock("@solidjs/router", () => ({
  useNavigate: () => navigate,
  useParams: () => ({ space_id: "default", entry_id: "entry-1" }),
}));
vi.mock("~/components/SpaceShell", () => ({
  SpaceShell: (props: { children: unknown }) => <div>{props.children}</div>,
}));
vi.mock("~/components/EntryDetailPane", () => ({
  EntryDetailPane: () => <div>Entry detail</div>,
}));

function renderRoute(formsError?: unknown) {
  render(() => (
    <EntriesRouteContext.Provider
      value={{
        spaceId: () => "default",
        forms: () => [],
        loadingForms: () => false,
        formsError: () => formsError,
        columnTypes: () => [],
        refetchForms,
        entryStore: {} as never,
        spaceStore: {} as never,
      }}
    >
      <SpaceEntryDetailRoute />
    </EntriesRouteContext.Provider>
  ));
}

describe("/spaces/:space_id/entries/:entry_id", () => {
  beforeEach(() => {
    setLocale("en");
    navigate.mockReset();
    refetchForms.mockReset();
  });

  it("surfaces form metadata failures with a retry action", () => {
    renderRoute(new Error("Forbidden"));

    expect(screen.getByText("Failed to load Forms.")).toBeInTheDocument();
    expect(screen.queryByText("Entry detail")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(refetchForms).toHaveBeenCalled();
  });
});
