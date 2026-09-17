import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import SpaceEntryRestoreRedirectRoute from "./restore";

const navigate = vi.fn();
const useParamsMock = vi.fn(() => ({
  space_id: "default",
  entry_id: "entry-1",
}));

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
  ),
  useNavigate: () => navigate,
  useParams: () => useParamsMock(),
}));

describe("legacy entry restore route", () => {
  beforeEach(() => {
    navigate.mockReset();
    useParamsMock.mockReset();
    useParamsMock.mockReturnValue({
      space_id: "default",
      entry_id: "entry-1",
    });
    setLocale("en");
  });

  it("redirects the compat /restore URL to the single History path", () => {
    render(() => <SpaceEntryRestoreRedirectRoute />);

    expect(navigate).toHaveBeenCalledWith(
      "/spaces/default/entries/entry-1/history",
      { replace: true },
    );
    const backLink = screen.getByRole("link", { name: "Back to history" });
    expect(backLink).toHaveAttribute(
      "href",
      "/spaces/default/entries/entry-1/history",
    );
    expect(screen.getByRole("status")).toHaveTextContent("Loading history...");
  });

  it("encodes the Space segment at its boundary", () => {
    useParamsMock.mockReturnValue({
      space_id: "team/東京",
      entry_id: "entry 1",
    });
    render(() => <SpaceEntryRestoreRedirectRoute />);

    const encoded = "/spaces/team%2F%E6%9D%B1%E4%BA%AC/entries/entry%201/history";
    expect(navigate).toHaveBeenCalledWith(encoded, { replace: true });
    const backLink = screen.getByRole("link", { name: "Back to history" });
    expect(backLink).toHaveAttribute("href", encoded);
  });
});
