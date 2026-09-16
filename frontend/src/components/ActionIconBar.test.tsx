import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { ActionIconBar } from "./ActionIconBar";

describe("ActionIconBar", () => {
  it("renders short-labelled actions with 44px tool targets", () => {
    const onRefresh = vi.fn();
    const { container } = render(() => (
      <ActionIconBar
        label="Entry actions"
        actions={[
          { id: "refresh", icon: "refresh", label: "更新", onClick: onRefresh },
          { id: "history", icon: "history", label: "履歴", href: "/history" },
          { id: "info", icon: "info", label: "情報" },
          { id: "delete", icon: "trash", label: "削除", danger: true },
        ]}
      />
    ));

    const bar = container.querySelector(".actionbar.compact-actions");
    expect(bar).toBeInTheDocument();
    expect(bar).toHaveAttribute("aria-label", "Entry actions");
    expect(screen.getByRole("button", { name: "更新" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "履歴" })).toHaveAttribute(
      "href",
      "/history",
    );
    for (const tool of container.querySelectorAll(".tool")) {
      expect(tool).toBeInTheDocument();
    }

    fireEvent.click(screen.getByRole("button", { name: "更新" }));
    expect(onRefresh).toHaveBeenCalledOnce();
  });

  it("warns when a free-form long label is passed", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    render(() => (
      <ActionIconBar
        actions={[
          {
            id: "long",
            icon: "info",
            label: "This label is far too long for a tile",
          },
        ]}
      />
    ));
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });
});
