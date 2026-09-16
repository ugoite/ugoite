import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import { LocalBusyIndicator } from "./LocalBusyIndicator";

describe("LocalBusyIndicator", () => {
  it("exposes role=status with the label as its content", () => {
    render(() => <LocalBusyIndicator label="Loading entries" />);

    // The accessible name comes from the sr-only content (no aria-label, so
    // labelled-control queries keep resolving to the real control). Real
    // assistive technology announces clipped content; this environment's
    // name computation does not, so assert the DOM contract instead.
    const status = screen.getByRole("status");
    expect(status).toBeInTheDocument();
    expect(status).toHaveClass("localpending");
    expect(status).toHaveTextContent("Loading entries");
  });

  it("keeps the label visually hidden while exposing it to AT", () => {
    const { container } = render(() => (
      <LocalBusyIndicator label="Loading entries" />
    ));

    const spinner = container.querySelector(".localspinner");
    expect(spinner).toBeInTheDocument();
    expect(spinner).toHaveAttribute("aria-hidden", "true");

    const srLabel = container.querySelector(".ui-sr-only");
    expect(srLabel).toHaveTextContent("Loading entries");
  });

  it("renders no visible loading text", () => {
    const { container } = render(() => (
      <LocalBusyIndicator label="Loading entries" />
    ));

    // The label travels via aria-label + the sr-only child only: no direct
    // (visible) text node may exist under the status element.
    const status = container.querySelector(".localpending");
    expect(status).toBeInTheDocument();
    const directTextNodes = [...(status?.childNodes ?? [])].filter(
      (node) =>
        node.nodeType === 3 && (node.textContent?.trim() ?? "") !== "",
    );
    expect(directTextNodes).toHaveLength(0);
    expect(status?.querySelector(".ui-sr-only")).toHaveTextContent(
      "Loading entries",
    );
  });

  it("supports the small footer size", () => {
    const { container } = render(() => (
      <LocalBusyIndicator size="sm" label="Loading more" />
    ));

    expect(container.querySelector(".localpending-sm")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Loading more");
  });
});
