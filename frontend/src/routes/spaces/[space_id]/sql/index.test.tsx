import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import SpaceSqlRoute from "./index";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>
      {props.children}
    </a>
  ),
  useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/components/SpaceShell", () => ({
  SpaceShell: (
    props: { children: unknown; spaceId: string; activeNavigation?: string },
  ) => (
    <div
      data-space-id={props.spaceId}
      data-active-navigation={props.activeNavigation}
    >
      {props.children}
    </div>
  ),
}));
vi.mock("~/lib/ugoite-client", () => ({
  sqlApi: { list: vi.fn().mockResolvedValue([]) },
}));

describe("/spaces/:space_id/sql", () => {
  it("REQ-FE-061: saved SQL route provides the v5 list and create action", async () => {
    const { container } = render(() => <SpaceSqlRoute />);

    expect(screen.getByRole("heading", { name: "Saved SQL" }))
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: "SQL" })).toHaveAttribute(
      "href",
      "/spaces/default/queries/new",
    );
    expect(container.firstElementChild).toHaveAttribute(
      "data-space-id",
      "default",
    );
    expect(container.firstElementChild).toHaveAttribute(
      "data-active-navigation",
      "search",
    );
  });
});
