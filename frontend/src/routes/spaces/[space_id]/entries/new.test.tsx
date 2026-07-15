import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Form, Space } from "~/lib/types";
import { formApi, spaceApi } from "~/lib/ugoite-client";
import NewEntryRoute from "./new";

const searchParams: Record<string, string> = {};

vi.mock("@solidjs/router", () => ({
  useNavigate: () => vi.fn(),
  useParams: () => ({ space_id: "default" }),
  useSearchParams: () => [searchParams, vi.fn()],
}));
vi.mock("~/components/SpaceShell", () => ({
  SpaceShell: (props: { children: unknown }) => <div>{props.children}</div>,
}));
vi.mock("~/components/create-dialogs", () => ({
  CreateEntryDialog: (props: { defaultForm?: string }) => (
    <div>Default form: {props.defaultForm}</div>
  ),
}));
vi.mock("~/lib/entry-store", () => ({
  createEntryStore: () => ({ createEntry: vi.fn() }),
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
    for (const key of Object.keys(searchParams)) delete searchParams[key];
    vi.mocked(formApi.list).mockResolvedValue(forms);
    vi.mocked(spaceApi.get).mockResolvedValue(space);
  });

  it("prefers the Form requested by the route", async () => {
    searchParams.form = "Meeting";
    render(() => <NewEntryRoute />);
    await waitFor(() =>
      expect(screen.getByText("Default form: Meeting")).toBeInTheDocument()
    );
  });

  it("falls back to the configured default for an unknown Form", async () => {
    searchParams.form = "Missing";
    render(() => <NewEntryRoute />);
    await waitFor(() =>
      expect(screen.getByText("Default form: Notes")).toBeInTheDocument()
    );
  });
});
