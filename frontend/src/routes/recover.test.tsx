import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import RecoverRoute from "./recover";

describe("/recover continuation", () => {
  it("explains that recovery is future functionality", () => {
    render(() => <RecoverRoute />);
    expect(screen.getByText("Passkey recovery is not available"))
      .toBeInTheDocument();
  });
});
