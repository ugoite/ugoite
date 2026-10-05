import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { ResultPagination } from "./ResultPagination";

const renderPagination = (overrides?: {
  canPrevious?: boolean;
  canNext?: boolean;
  busy?: boolean;
}) => {
  const onPrevious = vi.fn();
  const onNext = vi.fn();
  render(() => (
    <ResultPagination
      canPrevious={overrides?.canPrevious ?? false}
      canNext={overrides?.canNext ?? false}
      busy={overrides?.busy ?? false}
      previousLabel="Previous"
      nextLabel="Next"
      ariaLabel="Result pages"
      onPrevious={onPrevious}
      onNext={onNext}
    />
  ));
  return { onPrevious, onNext };
};

describe("ResultPagination", () => {
  it("renders no navigation for a single page", () => {
    renderPagination({ canPrevious: false, canNext: false });
    expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("moves to the previous page through the shared control", () => {
    const { onPrevious, onNext } = renderPagination({
      canPrevious: true,
      canNext: false,
    });
    const nav = screen.getByRole("navigation", { name: "Result pages" });
    expect(nav).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(onPrevious).toHaveBeenCalledOnce();
    expect(onNext).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
  });

  it("moves to the next page through the shared control", () => {
    const { onPrevious, onNext } = renderPagination({
      canPrevious: false,
      canNext: true,
    });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(onNext).toHaveBeenCalledOnce();
    expect(onPrevious).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Previous" })).toBeDisabled();
  });

  it("disables both controls while busy", () => {
    renderPagination({
      canPrevious: true,
      canNext: true,
      busy: true,
    });
    expect(screen.getByRole("button", { name: "Previous" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
  });
});
