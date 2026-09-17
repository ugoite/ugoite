import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { ActionIconBar } from "./ActionIconBar";

describe("ActionIconBar", () => {
  it("renders short-labelled actions with 44px tool targets", () => {
    setLocale("en");
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

  it("omits an unavailable nav target instead of a disabled link", () => {
    const { container } = render(() => (
      <ActionIconBar
        label="Entry actions"
        actions={[
          { id: "missing", icon: "history", label: "履歴", href: "/history", disabled: true },
          { id: "delete", icon: "trash", label: "削除", onClick: () => {} },
        ]}
      />
    ));

    expect(container.querySelector('a[href="/history"]')).toBeNull();
    expect(screen.getByRole("button", { name: "削除" })).toBeInTheDocument();
  });

  it("renders a true disabled button for disabled actions", () => {
    render(() => (
      <ActionIconBar
        actions={[
          { id: "delete", icon: "trash", label: "削除", disabled: true },
        ]}
      />
    ));

    const button = screen.getByRole("button", { name: "削除" });
    expect(button).toBeDisabled();
    expect(button.tagName).toBe("BUTTON");
  });

  it("keeps the interactive child as the accessible name owner", () => {
    const { container } = render(() => (
      <ActionIconBar
        actions={[{ id: "info", icon: "info", label: "情報" }]}
      />
    ));

    const label = container.querySelector(".toolLabel");
    expect(label).not.toHaveAttribute("aria-label");
    expect(screen.getByRole("button", { name: "情報" })).toBeInTheDocument();
  });

  it("passes in Japanese as well as English", () => {
    setLocale("ja");
    render(() => (
      <ActionIconBar
        label="エントリー操作"
        actions={[
          { id: "refresh", icon: "refresh", label: "更新" },
          { id: "history", icon: "history", label: "履歴", href: "/history" },
        ]}
      />
    ));

    expect(screen.getByRole("button", { name: "更新" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "履歴" })).toBeInTheDocument();
    setLocale("en");
  });
});
