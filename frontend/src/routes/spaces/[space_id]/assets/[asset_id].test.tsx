import "@testing-library/jest-dom/vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { assetApi } from "~/lib/ugoite-client";
import type { AssetListItem } from "~/lib/asset-api";
import SpaceAssetDetailRoute from "./[asset_id]";

const navigateMock = vi.fn();

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
  useNavigate: () => navigateMock,
  useParams: () => ({
    space_id: "default",
    asset_id: "01900000-0000-7000-8000-000000000001",
  }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  assetApi: { list: vi.fn(), read: vi.fn(), delete: vi.fn() },
}));

const item = (
  overrides: Partial<AssetListItem> = {},
): AssetListItem => ({
  asset_id: "01900000-0000-7000-8000-000000000001",
  name: "report.pdf",
  media_type: "application/pdf",
  size_bytes: 2048,
  sha256: "a".repeat(64),
  form: "Reports",
  entry_id: "entry-1",
  field: "Attachments",
  ...overrides,
});

describe("/spaces/:space_id/assets/:asset_id", () => {
  beforeEach(() => {
    setLocale("en");
    navigateMock.mockReset();
    vi.mocked(assetApi.list).mockReset();
    vi.mocked(assetApi.read).mockReset();
    vi.mocked(assetApi.delete).mockReset();
  });

  it("PR6: shows type/size meta with download/delete actions and entry references", async () => {
    vi.mocked(assetApi.list).mockResolvedValue([
      item(),
      item({ entry_id: "entry-2", field: "Cover" }),
    ]);

    render(() => <SpaceAssetDetailRoute />);

    expect(await screen.findByRole("heading", { name: "report.pdf" }))
      .toBeInTheDocument();
    expect(screen.getByText(/application\/pdf/)).toBeInTheDocument();
    // Single back control to the inventory.
    const back = screen.getByRole("link", { name: "Back to Assets" });
    expect(back).toHaveAttribute("href", "/spaces/default/assets");
    expect(screen.getAllByRole("link", { name: "Back to Assets" }))
      .toHaveLength(1);
    // Download/delete action bar. Delete is BLOCKED while visible Entry
    // references exist: disabled, naming the referencing Entry+field.
    const blockedDelete = screen.getByRole("button", {
      name:
        "Delete is blocked: this asset is still referenced by Reports · Attachments (entry-1), Reports · Cover (entry-2).",
    });
    expect(blockedDelete).toBeDisabled();
    expect(screen.getByRole("note")).toHaveTextContent(
      "Reports · Attachments (entry-1)",
    );
    expect(
      screen.queryByRole("button", { name: "Delete Asset: report.pdf" }),
    ).toBeNull();
    // References resolve to their owning Entries.
    const references = await screen.findAllByRole("link", {
      name: /Reports/,
    });
    expect(references).toHaveLength(2);
    expect(references[0]).toHaveAttribute(
      "href",
      "/spaces/default/entries/entry-1",
    );
    expect(references[1]).toHaveAttribute(
      "href",
      "/spaces/default/entries/entry-2",
    );
    // Exact IDs stay advanced-only inside the disclosure.
    expect(
      screen.getByText("01900000-0000-7000-8000-000000000001").closest(
        "details",
      ),
    ).not.toBeNull();
  });

  it("PR6: blocks delete while a visible Entry references the asset", async () => {
    vi.mocked(assetApi.list).mockResolvedValue([item()]);
    vi.mocked(assetApi.delete).mockResolvedValue({
      status: "deleted",
      id: "01900000-0000-7000-8000-000000000001",
    });

    render(() => <SpaceAssetDetailRoute />);

    const blockedDelete = await screen.findByRole("button", {
      name:
        "Delete is blocked: this asset is still referenced by Reports · Attachments (entry-1).",
    });
    expect(blockedDelete).toBeDisabled();
    // A disabled destructive action never fires, even on direct activation.
    fireEvent.click(blockedDelete);
    expect(assetApi.delete).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(navigateMock).not.toHaveBeenCalled();
  });

  it("PR6: deletes an unreferenced asset behind a confirmation", async () => {
    vi.mocked(assetApi.list).mockResolvedValue([]);
    vi.mocked(assetApi.delete).mockResolvedValue({
      status: "deleted",
      id: "01900000-0000-7000-8000-000000000001",
    });

    render(() => <SpaceAssetDetailRoute />);

    expect(
      await screen.findByText("No current Entry references this asset."),
    ).toBeInTheDocument();
    const remove = await screen.findByRole("button", {
      name: "Delete Asset",
    });
    expect(remove).toBeEnabled();
    fireEvent.click(remove);

    // Zero visible refs: confirmation first, server delete only on confirm.
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveAccessibleName("Delete Asset");
    expect(assetApi.delete).not.toHaveBeenCalled();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Delete Asset" }),
    );

    await waitFor(() => {
      expect(assetApi.delete).toHaveBeenCalledWith(
        "default",
        "01900000-0000-7000-8000-000000000001",
      );
      expect(navigateMock).toHaveBeenCalledWith("/spaces/default/assets");
    });
  });
});
