import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import SpaceEntryRestoreRedirectRoute from "./restore";

const navigate = vi.fn();

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
  ),
  useNavigate: () => navigate,
  useParams: () => ({ space_id: "default", entry_id: "entry-1" }),
}));

describe("legacy entry restore route", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
  });

  it("redirects the compat /restore URL to the single History path", () => {
    render(() => <SpaceEntryRestoreRedirectRoute />);

    expect(navigate).toHaveBeenCalledWith(
      "/spaces/default/entries/entry-1/history",
      { replace: true },
    );
    expect(screen.getByRole("link")).toHaveAttribute(
      "href",
      "/spaces/default/entries/entry-1/history",
    );
  });
});
