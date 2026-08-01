import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import SpaceLayout, {
  routeNavigation,
  routeTitle,
} from "./[space_id]";

vi.mock("@solidjs/router", () => ({
  useLocation: () => ({
    pathname: "/spaces/demo/entries/new",
    search: "?form=Notes",
  }),
  useParams: () => ({ space_id: "demo" }),
}));
vi.mock("~/components/SpaceShell", () => ({
  SpaceShell: (props: {
    activeNavigation?: string;
    title?: string;
    children: unknown;
  }) => (
    <section
      data-active-navigation={props.activeNavigation}
      data-title={props.title}
    >
      {props.children}
    </section>
  ),
}));

describe("SpaceLayout", () => {
  it("keeps the shared shell around entry creation", () => {
    render(() => (
      <SpaceLayout>
        <p>Entry editor</p>
      </SpaceLayout>
    ));

    const shell = screen.getByText("Entry editor").parentElement;
    expect(shell).toHaveAttribute("data-active-navigation", "forms");
    expect(shell).toHaveAttribute("data-title", "New Entry");
  });

  it("maps space routes to the persistent navigation", () => {
    expect(routeNavigation("/spaces/demo/forms")).toBe("forms");
    expect(routeNavigation("/spaces/demo/queries/new")).toBe("search");
    expect(routeNavigation("/spaces/demo/settings")).toBe("settings");
  });

  it("retains route-specific topbar titles", () => {
    expect(routeTitle("/spaces/demo/entries/one/history", ""))
      .toBe("Entry / History");
    expect(routeTitle("/spaces/demo/settings", "?section=storage"))
      .toBe("Settings / Storage");
  });
});
