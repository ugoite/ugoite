import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AssetField } from "./AssetField";
import { setLocale } from "~/lib/i18n";
import { assetApi } from "~/lib/ugoite-client";
import {
  serializeAssetReference,
  serializeAssetReferenceList,
} from "~/lib/asset-reference";
import type { AssetReference } from "~/lib/types";

vi.mock("~/lib/ugoite-client", () => ({
  assetApi: {
    upload: vi.fn(),
    read: vi.fn(),
  },
}));

const first: AssetReference = {
  asset_id: "01900000-0000-7000-8000-000000000001",
  name: "first.txt",
  media_type: "text/plain",
  size_bytes: 10,
  sha256: "a".repeat(64),
};

const second: AssetReference = {
  asset_id: "01900000-0000-7000-8000-000000000002",
  name: "second.txt",
  media_type: "text/plain",
  size_bytes: 20,
  sha256: "b".repeat(64),
};

beforeEach(() => {
  vi.resetAllMocks();
  setLocale("en");
});

describe("AssetField", () => {
  it("uploads a scalar reference without claiming Entry persistence", async () => {
    (assetApi.upload as ReturnType<typeof vi.fn>).mockResolvedValue(first);
    const [value, setValue] = createSignal("");

    render(() => (
      <AssetField
        fieldId="thumbnail"
        fieldName="thumbnail"
        value={value()}
        persistedValue=""
        multiple={false}
        spaceId="default"
        onChange={setValue}
        onPendingChange={vi.fn()}
      />
    ));

    fireEvent.change(screen.getByLabelText("Choose file"), {
      target: {
        files: [new File(["data"], "first.txt", { type: "text/plain" })],
      },
    });

    await waitFor(() => expect(value()).toBe(serializeAssetReference(first)));
    expect(screen.getByText("Uploaded; entry not saved yet"))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Remove" })).toBeInTheDocument();
  });

  it("keeps typed-list order and removes only the selected reference", () => {
    const onChange = vi.fn();
    const [value, setValue] = createSignal(
      serializeAssetReferenceList([first, second]),
    );
    const change = (next: string) => {
      setValue(next);
      onChange(next);
    };

    render(() => (
      <AssetField
        fieldId="documents"
        fieldName="documents"
        value={value()}
        persistedValue={serializeAssetReferenceList([first, second])}
        multiple={true}
        spaceId="default"
        entryId="entry-1"
        formName="Contracts"
        onChange={change}
        onPendingChange={vi.fn()}
      />
    ));

    fireEvent.click(screen.getAllByRole("button", { name: / up$/ })[1]);
    expect(onChange).toHaveBeenLastCalledWith(
      serializeAssetReferenceList([second, first]),
    );
    fireEvent.click(screen.getAllByRole("button", { name: /^Remove/ })[0]);
    expect(onChange).toHaveBeenLastCalledWith(
      serializeAssetReferenceList([first]),
    );
  });
});
