import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import AboutRoute from "./about";
vi.mock(
  "~/lib/ugoite-client",
  () => ({
    authApi: {
      getSession: vi.fn().mockResolvedValue({ authenticated: false }),
    },
  }),
);
describe("concept public pages", () => {
  beforeEach(() => setLocale("en"));
  it("renders About inside the new global shell and localizes it", async () => {
    render(() => <AboutRoute />);
    expect(screen.getByRole("heading", { name: "About Ugoite" }))
      .toBeInTheDocument();
    expect(screen.getAllByText("Ugoite").length).toBeGreaterThan(0);
    setLocale("ja");
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Ugoite について" }))
        .toBeInTheDocument()
    );
  });
});
