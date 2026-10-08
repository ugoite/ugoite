import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, within } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { entryApi } from "~/lib/ugoite-client";
import SpaceEntryInfoRoute from "~/routes/spaces/[space_id]/entries/[entry_id]/info";

vi.mock("@solidjs/router", () => ({
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    "aria-label"?: string;
    title?: string;
  }) => (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
      title={props.title}
    >
      {props.children}
    </a>
  ),
  useParams: () => ({ space_id: "default", entry_id: "entry-1" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  entryApi: { get: vi.fn() },
}));

vi.mock("~/components/AccessPolicyEditor", () => ({
  AccessPolicyEditor: () => <div>Access policy</div>,
}));

describe("Entry Info identity disclosure", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
    vi.mocked(entryApi.get).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: "Meeting",
      revision_id: "rev-9",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-02-02T00:00:00Z",
    });
  });

  it("REQ-UX-FORMTABLE-001: keeps Entry IDs in advanced Info details", async () => {
    render(() => <SpaceEntryInfoRoute />);

    const advancedDetails = (await screen.findByText("Advanced details"))
      .closest("details");
    if (!advancedDetails) {
      throw new Error("Advanced Entry details were not rendered");
    }

    const entryId = within(advancedDetails).getByText("entry-1");
    expect(entryId).not.toBeVisible();
    fireEvent.click(within(advancedDetails).getByText("Advanced details"));
    expect(entryId).toBeVisible();
    expect(
      within(advancedDetails).getByRole("button", { name: "Copy entry-1" }),
    ).toBeVisible();
  });
});
