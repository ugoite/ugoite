import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import NotFoundRoute from "./[...404]";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>
      {props.children}
    </a>
  ),
  useNavigate: () => vi.fn(),
  useParams: () => ({}),
}));

describe("404 route", () => {
  it("REQ-E2E-004: unknown routes keep shell navigation and alternate recovery paths", () => {
    render(() => <NotFoundRoute />);

    expect(screen.getByRole("heading", { name: "Page not found" }))
      .toBeInTheDocument();
    const spacesLink = screen.getAllByRole("link", { name: "Spaces" })
      .find((link) => link.getAttribute("href") === "/spaces");
    expect(spacesLink).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute(
      "href",
      "/login",
    );
    expect(screen.getByRole("link", { name: "Home" })).toHaveAttribute(
      "href",
      "/",
    );
    expect(screen.getByRole("link", { name: "Docs" })).toHaveAttribute(
      "href",
      "https://ugoite.github.io/ugoite/docs/get-started",
    );
  });
});
