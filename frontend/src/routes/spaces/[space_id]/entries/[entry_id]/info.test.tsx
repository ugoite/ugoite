import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { formatDateTimeLabel } from "~/lib/date-format";
import { entryApi } from "~/lib/ugoite-client";
import SpaceEntryInfoRoute from "./info";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
  ),
  useParams: () => ({ space_id: "default", entry_id: "entry-1" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  entryApi: { get: vi.fn() },
}));

vi.mock("~/components/AccessPolicyEditor", () => ({
  AccessPolicyEditor: () => <div>Access policy</div>,
}));

describe("entry info route", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
  });

  it("renders metadata values with a back link", async () => {
    vi.mocked(entryApi.get).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: "Meeting",
      content: "# Test Entry",
      revision_id: "rev-9",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-02-02T00:00:00Z",
    });

    render(() => <SpaceEntryInfoRoute />);

    expect(await screen.findByText("entry-1")).toBeInTheDocument();
    expect(screen.getByText("Meeting")).toBeInTheDocument();
    expect(
      screen.getByText(formatDateTimeLabel("2026-02-02T00:00:00Z")),
    ).toBeInTheDocument();
    expect(
      screen.getByText(formatDateTimeLabel("2026-01-01T00:00:00Z")),
    ).toBeInTheDocument();
    expect(screen.getByText("rev-9")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to Entry" }))
      .toHaveAttribute("href", "/spaces/default/entries/entry-1");
    expect(screen.getByRole("button", { name: "Copy entry-1" }))
      .toBeInTheDocument();
    expect(document.querySelector(".eyebrow")).toHaveClass("break-all");
    // Info action on the detail page targets this route.
    expect(screen.queryByText(/Markdown/)).not.toBeInTheDocument();
  });

  it("surfaces load errors", async () => {
    vi.mocked(entryApi.get).mockRejectedValue(new Error("boom"));

    render(() => <SpaceEntryInfoRoute />);

    expect(await screen.findByText(/Failed to load entry info/))
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to Entry" }))
      .toBeInTheDocument();
  });

  it("renders a not-found state when the entry is missing", async () => {
    vi.mocked(entryApi.get).mockResolvedValue(null);

    render(() => <SpaceEntryInfoRoute />);

    expect(await screen.findByText("Entry not found.")).toBeInTheDocument();
  });
});
