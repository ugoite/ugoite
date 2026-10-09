import { describe, expect, it } from "vitest";
import { createClientErrorDiagnostic } from "./client-error-diagnostics";

describe("client error diagnostics", () => {
  it("keeps only an allowlisted type, route family, and source positions", () => {
    const error = new Error(
      "Private Form title 4546c198-284b-4f53-9a11-c51edc899004 token=secret",
    );
    error.stack = [
      error.message,
      "    at load (https://tenant.example/spaces/4546c198-284b-4f53-9a11-c51edc899004/forms/private-entry-name.js?access_token=secret:48:12)",
      "    at https://tenant.example/assets/private-entry-name.js:72:8",
    ].join("\n");

    const diagnostic = createClientErrorDiagnostic(
      error,
      "/spaces/4546c198-284b-4f53-9a11-c51edc899004/forms/Private%20Form%20title/entries?access_token=secret",
    );
    const serialized = JSON.stringify(diagnostic);

    expect(diagnostic).toEqual({
      category: "app-error-boundary",
      errorType: "Error",
      fingerprint: expect.any(String),
      routeFamily: "/spaces/:space/forms",
      stackLocations: ["javascript:48:12", "javascript:72:8"],
    });
    for (const secret of [
      "tenant.example",
      "4546c198-284b-4f53-9a11-c51edc899004",
      "Private Form title",
      "private-entry-name",
      "access_token",
      "secret",
    ]) {
      expect(serialized).not.toContain(secret);
    }
  });

  it("fingerprints error type and normalized source positions, not messages", () => {
    const first = new TypeError("first private message");
    const second = new TypeError("second private message");
    first.stack = "TypeError: first private message\n    at load (https://host.invalid/assets/a.js:10:4)";
    second.stack = "TypeError: second private message\n    at load (https://another.invalid/assets/b.js:10:4)";

    expect(createClientErrorDiagnostic(first, "/spaces/one/forms").fingerprint)
      .toBe(createClientErrorDiagnostic(second, "/spaces/two/forms").fingerprint);
  });

  it("does not trust arbitrary error names", () => {
    const error = new Error("private message");
    Object.defineProperty(error, "stack", { value: error.stack });
    let nameReads = 0;
    Object.defineProperty(error, "name", {
      get: () => nameReads++ === 0 ? "TypeError" : "PrivateErrorWithData",
    });

    expect(createClientErrorDiagnostic(error, "/settings").errorType)
      .toBe("TypeError");
    expect(nameReads).toBe(1);
  });
});
