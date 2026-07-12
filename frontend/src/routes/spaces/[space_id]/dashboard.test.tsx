import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import SpaceDashboardRoute from "./dashboard";
import { formApi, spaceApi } from "~/lib/ugoite-client";
import { setLocale } from "~/lib/i18n";

vi.mock("@solidjs/router", () => ({
  useNavigate: () => vi.fn(),
  useParams: () => ({ space_id: "default" }),
  A: (props: { href: string; class?: string; children: unknown }) => <a href={props.href} class={props.class}>{props.children}</a>,
}));
vi.mock("~/components/SpaceShell", () => ({ SpaceShell: (props: { children: unknown }) => <div>{props.children}</div> }));
vi.mock("~/lib/entry-store", () => ({ createEntryStore: () => ({ createEntry: vi.fn() }) }));
vi.mock("~/lib/ugoite-client", () => ({
  formApi: { list: vi.fn(), listTypes: vi.fn(), create: vi.fn() },
  spaceApi: { get: vi.fn() },
}));

describe("/spaces/:space_id/dashboard", () => {
  beforeEach(() => {
    setLocale("en");
    (spaceApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({ id: "default", name: "Default Space", settings: { default_form: "Meeting" } });
    (formApi.listTypes as ReturnType<typeof vi.fn>).mockResolvedValue([]);
    (formApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([{ name: "Meeting", version: 1, template: "", fields: {} }]);
  });

  it("uses the v5 Home information architecture", async () => {
    render(() => <SpaceDashboardRoute />);
    await waitFor(() => expect(screen.getByRole("heading", { name: "Home" })).toBeInTheDocument());
    expect(screen.getByRole("heading", { name: "Continue" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Pinned" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Recent" })).toBeInTheDocument();
    expect(screen.queryByText("Storage topology")).not.toBeInTheDocument();
  });

  it("starts a form-selected entry flow from Home", async () => {
    render(() => <SpaceDashboardRoute />);
    const button = await screen.findByRole("button", { name: "New entry" });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    await waitFor(() => expect(screen.getByRole("heading", { name: "Create New Entry" })).toBeInTheDocument());
    expect(screen.getByDisplayValue("Meeting")).toBeInTheDocument();
  });
});
