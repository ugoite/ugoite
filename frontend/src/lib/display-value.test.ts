import { describe, expect, it } from "vitest";
import {
  formatAssetSummary,
  formatValueForDisplay,
  formatValueForInput,
  safeText,
} from "./display-value";

const asset = {
  asset_id: "asset-1",
  name: "hero-banner.png",
  media_type: "image/png",
  size_bytes: 1_800_000,
  sha256: "a".repeat(64),
} as const;

describe("display-value", () => {
  it("converts scalar values and rejects object coercion", () => {
    expect(safeText("text")).toBe("text");
    expect(safeText(0)).toBe("0");
    expect(safeText(false)).toBe("false");
    expect(safeText({ value: "hidden" })).toBe("");
    expect(safeText(["hidden"])).toBe("");
  });

  it("formats assets as type and size metadata", () => {
    expect(formatAssetSummary(asset)).toMatch(/^PNG · /);
    expect(formatValueForDisplay(asset)).toMatch(/^PNG · /);
  });

  it("keeps scalar lists readable and hides structured records", () => {
    expect(formatValueForDisplay(["one", "two"])).toBe("one, two");
    expect(formatValueForDisplay([{ value: "hidden" }])).toBe("-");
    expect(formatValueForInput(["one", "two"])).toBe("one, two");
    expect(formatValueForInput({ value: "hidden" })).toBe("");
  });
});
