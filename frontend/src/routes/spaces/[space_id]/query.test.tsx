import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import SpaceQueryRoute from "./query";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>
      {props.children}
    </a>
  ),
  useParams: () => ({ space_id: "default" }),
}));

describe("/spaces/:space_id/query", () => {
  it("REQ-FE-063: legacy query route explains the supported search path", () => {
    const { container } = render(() => <SpaceQueryRoute />);
    const subtitle = container.querySelector(".ui-page-subtitle");

    expect(screen.getByRole("heading", { name: "Query moved to Search" }))
      .toBeInTheDocument();
    expect(subtitle).not.toBeNull();
    expect(subtitle).toHaveTextContent(
      "This legacy /query URL is kept only so older bookmarks and stale links do not fail.",
    );
    expect(subtitle).toHaveTextContent("/search");
    expect(
      screen.getByText(/keyword search, advanced filters, and query history/i),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Open Search" })).toHaveAttribute(
      "href",
      "/spaces/default/search",
    );
    expect(screen.getByRole("link", { name: "Back to Dashboard" }))
      .toHaveAttribute(
        "href",
        "/spaces/default/dashboard",
      );
  });
});
