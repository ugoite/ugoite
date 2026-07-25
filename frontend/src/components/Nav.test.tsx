import "@testing-library/jest-dom/vitest";
import { render } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import Nav from "./Nav";

describe("Nav", () => {
  it("leaves navigation to the v5 page shells", () => {
    const { container } = render(() => <Nav />);
    expect(container.firstChild).toBeNull();
  });
});
