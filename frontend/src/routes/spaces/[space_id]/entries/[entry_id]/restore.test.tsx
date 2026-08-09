import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { formatDateTimeLabel } from "~/lib/date-format";
import { setLocale } from "~/lib/i18n";
import { entryApi } from "~/lib/ugoite-client";
import SpaceEntryRestoreRoute from "./restore";

const navigate = vi.fn();

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
  ),
  useNavigate: () => navigate,
  useParams: () => ({ space_id: "default", entry_id: "entry-1" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  entryApi: { history: vi.fn(), restore: vi.fn() },
}));

describe("entry restore route", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
  });

  it("renders a string revision timestamp", async () => {
    const timestamp = "2026-01-01T00:00:00Z";
    vi.mocked(entryApi.history).mockResolvedValue({
      revisions: [{
        revision_id: "rev-1",
        timestamp,
        checksum: "checksum",
      }],
    });

    render(() => <SpaceEntryRestoreRoute />);

    expect(await screen.findByText(formatDateTimeLabel(timestamp)))
      .toBeInTheDocument();
  });
});
