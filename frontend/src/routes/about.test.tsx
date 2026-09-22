import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@solidjs/testing-library";
import AboutRedirectRoute, { aboutDocsHref } from "./about";
import { setLocale } from "~/lib/i18n";

const docsHref =
  "https://ugoite.github.io/ugoite/docs/get-started";

describe("/about", () => {
  const originalLocation = window.location;
  const replaceMock = vi.fn();

  beforeEach(() => {
    localStorage.clear();
    setLocale("en");
    replaceMock.mockReset();
    Object.defineProperty(window, "location", {
      configurable: true,
      value: { ...originalLocation, replace: replaceMock },
    });
  });

  afterEach(() => {
    Object.defineProperty(window, "location", {
      configurable: true,
      value: originalLocation,
    });
    setLocale("en");
  });

  it("REQ-FE-064: redirects About deep links to the single Docs authority", () => {
    expect(aboutDocsHref).toBe(docsHref);

    render(() => <AboutRedirectRoute />);

    expect(replaceMock).toHaveBeenCalledWith(docsHref);
    expect(screen.getByRole("link", { name: "Docs" })).toHaveAttribute(
      "href",
      docsHref,
    );
  });

  it("renders no in-app About copy in Japanese", () => {
    setLocale("ja");

    render(() => <AboutRedirectRoute />);

    expect(replaceMock).toHaveBeenCalledWith(docsHref);
    expect(
      screen.queryByRole("heading", { name: "Ugoite について" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("このアプリについて")).not.toBeInTheDocument();
  });
});
