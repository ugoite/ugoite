import { describe, expect, it } from "vitest";
import { classifyMutationOutcome } from "./mutation-outcome";

describe("classifyMutationOutcome", () => {
  it("distinguishes rejected writes, unknown transport results, and bad receipts", () => {
    expect(classifyMutationOutcome({
      operation: "entry.update",
      status: 409,
      kind: "conflict",
      code: "REVISION_CONFLICT",
    })).toBe("rejected");
    expect(classifyMutationOutcome({
      operation: "asset.upload",
      kind: "transport",
    })).toBe("unknown");
    expect(classifyMutationOutcome({
      operation: "entry.update",
      status: 408,
      kind: "api",
    })).toBe("unknown");
    expect(classifyMutationOutcome({
      operation: "form.upsert",
      kind: "invalid_arguments",
    })).toBe("rejected");
    expect(classifyMutationOutcome({
      operation: "form.upsert",
      status: 200,
      kind: "invalid_response",
    })).toBe("receipt_invalid");
  });

  it("does not classify reads as mutation outcomes", () => {
    expect(classifyMutationOutcome({
      operation: "entry.query",
      status: 503,
      kind: "transport",
    })).toBeNull();
  });
});
