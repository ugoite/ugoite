import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { formatDateTimeLabel } from "~/lib/date-format";
import { entryApi } from "~/lib/ugoite-client";
import SpaceEntryHistoryRoute from "./index";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
  ),
  useParams: () => ({ space_id: "default", entry_id: "entry-1" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  entryApi: { history: vi.fn() },
}));

describe("entry history route", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
  });

  it("renders the backend revision timestamp", async () => {
    const timestamp = 1767225600;
    vi.mocked(entryApi.history).mockResolvedValue({
      revisions: [{
        revision_id: "rev-1",
        timestamp,
        checksum: "checksum",
      }],
    });

    render(() => <SpaceEntryHistoryRoute />);

    expect(await screen.findByText(formatDateTimeLabel(timestamp)))
      .toBeInTheDocument();
  });
});
