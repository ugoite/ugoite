import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import DeviceApprovalRoute from "./device";

describe("/device", () => {
  it("explains that device authorization is future functionality", () => {
    render(() => <DeviceApprovalRoute />);
    expect(screen.getByText("Device authorization is not available"))
      .toBeInTheDocument();
  });
});
