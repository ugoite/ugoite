import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import { IconLink } from "./IconLink";

describe("IconLink", () => {
  it("renders a link-only control that is never disabled", () => {
    const { container } = render(() => (
      <IconLink icon="spaces" label="Spaces" href="/spaces" />
    ));

    const link = screen.getByRole("link", { name: "Spaces" });
    expect(link).toHaveAttribute("href", "/spaces");
    expect(link).not.toHaveAttribute("disabled");
    expect(link).not.toHaveAttribute("aria-disabled");
    expect(container.querySelector(".icononly")).toBeInTheDocument();
  });
});
