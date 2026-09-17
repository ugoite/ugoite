import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { assetApi } from "~/lib/ugoite-client";
import type { AssetListItem } from "~/lib/asset-api";
import SpaceAssetsIndexRoute from "./assets/index";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
  ),
  useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  assetApi: { list: vi.fn(), read: vi.fn(), delete: vi.fn() },
}));

vi.mock("~/lib/user-facing-error", () => ({
  formatUserFacingError: (_error: unknown, fallback: string) =>
    fallback === "assetsPage.failedLoad" ? "Failed to load assets." : fallback,
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

describe("/spaces/:space_id/assets", () => {
  beforeEach(() => {
    setLocale("en");
    vi.mocked(assetApi.list).mockReset();
  });

  it("renders loading and then asset rows with type/size meta", async () => {
    let resolveItems: ((value: AssetListItem[]) => void) | undefined;
    vi.mocked(assetApi.list).mockReturnValue(
      new Promise((resolve) => resolveItems = resolve),
    );

    const { container } = render(() => <SpaceAssetsIndexRoute />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Loading asset references...",
    );

    resolveItems?.([item()]);

    expect(await screen.findByRole("link", { name: /report\.pdf/ }))
      .toHaveAttribute(
        "href",
        "/spaces/default/assets/01900000-0000-7000-8000-000000000001",
      );
    expect(assetApi.list).toHaveBeenCalledWith("default");
    expect(screen.getByText(/application\/pdf/)).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(
      "All current Entry asset references are shown.",
    );
    // RowList rows: full-row activation, no raw IDs in rows.
    expect(container.querySelector(".rowList")).toBeInTheDocument();
    expect(container.querySelector(".spaceAssetRow")).toBeNull();
    expect(screen.queryByText("01900000-0000-7000-8000-000000000001"))
      .toBeNull();
  });

  it("exposes an Upload action where bytes enter through asset fields", async () => {
    vi.mocked(assetApi.list).mockResolvedValue([]);

    render(() => <SpaceAssetsIndexRoute />);

    expect(await screen.findByText("No saved Asset references yet."))
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: "+Upload" }))
      .toHaveAttribute("href", "/spaces/default/forms");
    expect(screen.getByText(/never creates a second asset catalog/))
      .toBeInTheDocument();
  });

  it("renders an error with a retry action", async () => {
    vi.mocked(assetApi.list)
      .mockRejectedValueOnce(new Error("request failed"))
      .mockResolvedValueOnce([]);

    render(() => <SpaceAssetsIndexRoute />);

    expect(await screen.findByText("Failed to load assets."))
      .toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(assetApi.list).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("No saved Asset references yet."))
      .toBeInTheDocument();
  });
});
