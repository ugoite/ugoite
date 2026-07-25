import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it } from "vitest";
import { setLocale } from "~/lib/i18n";
import { AppErrorBoundary } from "./AppErrorBoundary";

function BrokenPage() {
  throw new Error("sensitive internal error");
}

describe("AppErrorBoundary", () => {
  beforeEach(() => setLocale("en"));

  it("replaces uncaught route errors with a recoverable page", () => {
    render(() => (
      <AppErrorBoundary>
        <BrokenPage />
      </AppErrorBoundary>
    ));

    expect(screen.getByRole("alert")).toHaveTextContent(
      "This page could not be displayed",
    );
    expect(screen.queryByText("sensitive internal error")).not
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to Spaces" }))
      .toHaveAttribute("href", "/spaces");
    expect(screen.getByRole("button", { name: "Try again" }))
      .toBeInTheDocument();
  });
});
