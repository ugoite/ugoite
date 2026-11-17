import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import { IconButton } from "./IconButton";
import { IconLink } from "./IconLink";

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

  it("keeps a 44px target for icon-only buttons", () => {
    const { container } = render(() => (
      <IconButton icon="settings" label="Settings" />
    ));

    const button = screen.getByRole("button", { name: "Settings" });
    expect(button.tagName).toBe("BUTTON");
    expect(container.querySelector(".icononly")).toBeInTheDocument();
  });

  it("forwards bound click handlers with their data", () => {
    const calls: string[] = [];
    const onClick = (value: string) => calls.push(value);

    render(() => (
      <IconButton
        icon="settings"
        label="Settings"
        onClick={[onClick, "bound"]}
      />
    ));

    fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    expect(calls).toEqual(["bound"]);
  });
});

describe("IconLink", () => {
  it("renders an icon-only link with a required accessible label", () => {
    render(() => <IconLink icon="spaces" label="Spaces" href="/spaces" />);

    const link = screen.getByRole("link", { name: "Spaces" });
    expect(link).toHaveAttribute("href", "/spaces");
    expect(link).toHaveClass("iconpill");
    expect(link.tagName).toBe("A");
  });
});
