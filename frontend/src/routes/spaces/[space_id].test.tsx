import "@testing-library/jest-dom/vitest";
import {
  A,
  createMemoryHistory,
  MemoryRouter,
  type RouteDefinition,
} from "@solidjs/router";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { spaceRoute } from "~/lib/space-shell-route";
import SpaceLayout, { route as spaceLayoutRoute } from "./[space_id]";

const { loadSpacesMock } = vi.hoisted(() => ({
  loadSpacesMock: vi.fn(),
}));

vi.mock("~/lib/space-store", () => ({
  createSpaceStore: () => ({
    spaces: () => [{ id: "demo", name: "Demo", created_at: "" }],
    loadSpaces: loadSpacesMock,
    selectSpace: vi.fn(),
  }),
}));

const formsRoute = spaceRoute({ navigation: "forms" });
const newEntryRoute = spaceRoute({ navigation: "forms", title: "newEntry" });
const testConnectionRoute = spaceRoute({
  navigation: "settings",
  title: "settingsStorage",
});

function FormsPage() {
  return (
    <>
      <p>Forms route</p>
      <A href="/spaces/demo/entries/new">New Entry</A>
      <A href="/spaces/demo/test-connection">Test connection</A>
    </>
  );
}

function NewEntryPage() {
  return <p>New Entry route</p>;
}

function TestConnectionPage() {
  return <p>Test connection route</p>;
}

const routes: RouteDefinition[] = [{
  path: "/spaces/:space_id",
  component: SpaceLayout,
  info: spaceLayoutRoute.info,
  children: [
    { path: "/forms", component: FormsPage, ...formsRoute },
    { path: "/entries/new", component: NewEntryPage, ...newEntryRoute },
    {
      path: "/test-connection",
      component: TestConnectionPage,
      ...testConnectionRoute,
    },
  ],
}];

function renderAt(path: string) {
  const history = createMemoryHistory();
  history.set({ value: path, replace: true, scroll: false });
  render(() => <MemoryRouter history={history}>{routes}</MemoryRouter>);
  return history;
}

describe("/spaces/:space_id persistent layout", () => {
  beforeEach(() => {
    setLocale("en");
    loadSpacesMock.mockReset();
    loadSpacesMock.mockResolvedValue(undefined);
  });

  it("keeps the same shell instance and loads spaces once across child navigation", async () => {
    renderAt("/spaces/demo/forms");
    const shell = screen.getByRole("main");

    expect(document.querySelector(".crumbTop")).toHaveTextContent("Forms");
    fireEvent.click(screen.getByRole("link", { name: "New Entry" }));

    await waitFor(() => {
      expect(screen.getByText("New Entry route")).toBeInTheDocument();
      expect(document.querySelector(".crumbTop")).toHaveTextContent(
        "New Entry",
      );
      expect(loadSpacesMock).toHaveBeenCalledOnce();
      expect(screen.getAllByRole("link", { name: "Forms" })[0])
        .toHaveClass("active");
      expect(screen.getAllByRole("link", { name: "Forms" })[0])
        .toHaveAttribute("aria-current", "page");
    });
    expect(screen.getByRole("main")).toBe(shell);
  });

  it("derives Settings navigation from the matched route metadata", async () => {
    renderAt("/spaces/demo/test-connection");

    await waitFor(() => {
      expect(document.querySelector(".crumbTop")).toHaveTextContent(
        "Settings / Storage",
      );
    });
    expect(screen.getAllByRole("link", { name: "Settings" })[0])
      .toHaveClass("active");
    expect(screen.getAllByRole("link", { name: "Home" })[0])
      .not.toHaveClass("active");
  });

  it("keeps Forms localized when the shell uses the route fallback title", () => {
    setLocale("ja");
    renderAt("/spaces/demo/forms");

    expect(document.querySelector(".crumbTop")).toHaveTextContent("フォーム");
  });

  it("uses route metadata when the space id contains navigation names", () => {
    renderAt("/spaces/search/forms");

    expect(screen.getAllByRole("link", { name: "Forms" })[0])
      .toHaveClass("active");
    expect(screen.getAllByRole("link", { name: "Search" })[0])
      .not.toHaveClass("active");
  });
});
