import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import { IconButton } from "./IconButton";

describe("IconButton", () => {
  it("renders an icon-only button with a required accessible label", () => {
    const { container } = render(() => (
      <IconButton icon="settings" label="Settings" />
    ));

    const button = screen.getByRole("button", { name: "Settings" });
    expect(button).toHaveClass("iconpill");
    expect(button).toHaveClass("icononly");
    const svg = container.querySelector("svg.icon");
    expect(svg).toBeInTheDocument();
    expect(svg).toHaveAttribute("viewBox", "0 0 24 24");
  });

  it("renders an anchor when href is given", () => {
    render(() => <IconButton icon="spaces" label="Spaces" href="/spaces" />);

    const link = screen.getByRole("link", { name: "Spaces" });
    expect(link).toHaveAttribute("href", "/spaces");
    expect(link).toHaveClass("iconpill");
  });
});
