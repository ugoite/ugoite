import { describe, expect, it } from "vitest";
import { calculateVisibleRowRange } from "./virtual-rows";

describe("calculateVisibleRowRange", () => {
  it("includes six rows of overscan around a 480px viewport", () => {
    expect(calculateVisibleRowRange(1_000, 0, 480)).toEqual({
      start: 0,
      end: 22,
      topSpacerHeight: 0,
      bottomSpacerHeight: 46_944,
    });
  });

  it("moves the visible window with scroll position", () => {
    expect(calculateVisibleRowRange(1_000, 4_800, 720)).toEqual({
      start: 94,
      end: 121,
      topSpacerHeight: 4_512,
      bottomSpacerHeight: 42_192,
    });
  });

  it("bounds the range at the end and handles an empty page", () => {
    expect(calculateVisibleRowRange(20, 48_000, 720)).toEqual({
      start: 0,
      end: 20,
      topSpacerHeight: 0,
      bottomSpacerHeight: 0,
    });
    expect(calculateVisibleRowRange(0, 0, 480)).toEqual({
      start: 0,
      end: 0,
      topSpacerHeight: 0,
      bottomSpacerHeight: 0,
    });
  });
});
