import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Form, Space } from "~/lib/types";
import { formApi, spaceApi } from "~/lib/ugoite-client";
import NewEntryRoute from "./new";

const searchParams: Record<string, string> = {};
const navigate = vi.fn();

vi.mock("@solidjs/router", () => ({
  useNavigate: () => navigate,
  useParams: () => ({ space_id: "default" }),
  useSearchParams: () => [searchParams, vi.fn()],
}));
vi.mock("~/components/SpaceShell", () => ({
  SpaceShell: (props: { children: unknown }) => <div>{props.children}</div>,
}));
vi.mock("~/components/EntryDetailPane", () => ({
  EntryDetailPane: (props: {
    createForm?: () => Form | undefined;
    onCreateFormChange?: (name: string) => void;
    onCreated?: (id: string) => void;
    onDeleted: () => void;
  }) => (
    <div>
      <p>Shared entry editor: {props.createForm?.()?.name}</p>
      <button
        type="button"
        onClick={() => props.onCreateFormChange?.("Meeting")}
      >
        Select Meeting
      </button>
      <button type="button" onClick={() => props.onCreated?.("entry-1")}>
        Save entry
      </button>
      <button type="button" onClick={props.onDeleted}>Cancel</button>
    </div>
  ),
}));
vi.mock("~/lib/ugoite-client", () => ({
  formApi: { list: vi.fn() },
  spaceApi: { get: vi.fn() },
}));

const forms: Form[] = [
  { name: "Notes", version: 1, template: "", fields: {} },
  { name: "Meeting", version: 1, template: "", fields: {} },
];
const space: Space = {
  id: "default",
  name: "Default",
  created_at: "2026-01-01T00:00:00Z",
  settings: { default_form: "Notes" },
};

describe("NewEntryRoute", () => {
  beforeEach(() => {
    navigate.mockReset();
    for (const key of Object.keys(searchParams)) delete searchParams[key];
    vi.mocked(formApi.list).mockResolvedValue(forms);
    vi.mocked(spaceApi.get).mockResolvedValue(space);
  });

  it("reuses the shared editor and prefers the Form requested by the route", async () => {
    searchParams.form = "Meeting";
    render(() => <NewEntryRoute />);
    await waitFor(() =>
      expect(screen.getByText("Shared entry editor: Meeting"))
        .toBeInTheDocument()
    );
  });

  it("falls back to the configured default for an unknown Form", async () => {
    searchParams.form = "Missing";
    render(() => <NewEntryRoute />);
    await waitFor(() =>
      expect(screen.getByText("Shared entry editor: Notes")).toBeInTheDocument()
    );
  });

  it("waits for both route resources before opening the editor", async () => {
    let resolveForms: (forms: Form[]) => void;
    let resolveSpace: (space: Space) => void;
    vi.mocked(formApi.list).mockReturnValue(
      new Promise<Form[]>((resolve) => {
        resolveForms = resolve;
      }),
    );
    vi.mocked(spaceApi.get).mockReturnValue(
      new Promise<Space>((resolve) => {
        resolveSpace = resolve;
      }),
    );

    render(() => <NewEntryRoute />);
    expect(screen.getByRole("status")).toHaveTextContent("Loading entry form");
    resolveForms!(forms);
    await waitFor(() => expect(screen.getByRole("status")).toBeInTheDocument());
    resolveSpace!(space);
    await waitFor(() =>
      expect(screen.getByText("Shared entry editor: Notes")).toBeInTheDocument()
    );
  });

  it("navigates to the created entry and returns to Forms on cancel", async () => {
    render(() => <NewEntryRoute />);
    fireEvent.click(await screen.findByRole("button", { name: "Save entry" }));
    expect(navigate).toHaveBeenCalledWith(
      "/spaces/default/entries/entry-1",
      { replace: true },
    );
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(navigate).toHaveBeenCalledWith("/spaces/default/forms");
  });
});
