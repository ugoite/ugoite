import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { entryApi } from "~/lib/ugoite-client";
import type { EntryRecord } from "~/lib/types";
import SpaceAssetsRoute from "./assets";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
  ),
  useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  entryApi: { list: vi.fn() },
}));

vi.mock("~/lib/asset-reference", () => ({
  formatAssetSize: (size: number) => `${size.toLocaleString("en-US")} bytes`,
  isAssetReference: (value: unknown) =>
    !!value && typeof value === "object" &&
    "asset_id" in value && "name" in value && "media_type" in value &&
    "size_bytes" in value && "sha256" in value,
}));

vi.mock("~/lib/user-facing-error", () => ({
  formatUserFacingError: (_error: unknown, fallback: string) =>
    fallback === "assetsPage.failedLoad" ? "Failed to load assets." : fallback,
}));

const reference = {
  asset_id: "01900000-0000-7000-8000-000000000001",
  name: "report.pdf",
  media_type: "application/pdf",
  size_bytes: 2048,
  sha256: "a".repeat(64),
};

const entry = (properties: Record<string, unknown>): EntryRecord => ({
  id: "entry-1",
  title: "Quarterly report",
  form: "Reports",
  updated_at: "2026-08-10T00:00:00Z",
  properties,
  tags: [],
});

describe("/spaces/:space_id/assets", () => {
  beforeEach(() => {
    setLocale("en");
    vi.mocked(entryApi.list).mockReset();
  });

  it("renders loading and then the Form-owned asset reference with its Entry", async () => {
    let resolveEntries: ((value: EntryRecord[]) => void) | undefined;
    vi.mocked(entryApi.list).mockReturnValue(
      new Promise((resolve) => resolveEntries = resolve),
    );

    render(() => <SpaceAssetsRoute />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Loading asset references...",
    );

    resolveEntries?.([entry({ Attachments: [reference] })]);

    expect(await screen.findByRole("heading", { name: "report.pdf" }))
      .toBeInTheDocument();
    expect(entryApi.list).toHaveBeenCalledWith("default", 10_000);
    expect(screen.getByText("application/pdf · 2,048 bytes"))
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Quarterly report/ }))
      .toHaveAttribute("href", "/spaces/default/entries/entry-1");
  });

  it("renders the honest empty state when no current Entry owns an asset", async () => {
    vi.mocked(entryApi.list).mockResolvedValue([]);

    render(() => <SpaceAssetsRoute />);

    expect(await screen.findByText("No saved Asset references yet."))
      .toBeInTheDocument();
    expect(screen.getByText(/Upload an asset in a Form-owned asset field/))
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Open Forms/ }))
      .toHaveAttribute("href", "/spaces/default/forms");
  });

  it("renders an error with a retry action", async () => {
    vi.mocked(entryApi.list)
      .mockRejectedValueOnce(new Error("request failed"))
      .mockResolvedValueOnce([]);

    render(() => <SpaceAssetsRoute />);

    expect(await screen.findByText("Failed to load assets."))
      .toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(entryApi.list).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("No saved Asset references yet."))
      .toBeInTheDocument();
  });
});
