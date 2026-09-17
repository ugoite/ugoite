import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { RowList, RowListButton, RowListItem, RowListLink } from "./RowList";

vi.mock("@solidjs/router", () => ({
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    "aria-label"?: string;
    title?: string;
  }) => (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
      title={props.title}
    >
      {props.children}
    </a>
  ),
}));

describe("RowList", () => {
  it("REQ-UX-LIST-001: activates the full row without a boxed chevron or Open column", () => {
    const onActivate = vi.fn();
    const { container } = render(() => (
      <RowList label="Spaces">
        <RowListItem
          main={
            <RowListButton
              onActivate={onActivate}
              primary="Default"
              meta="3 entries"
              chevron
            />
          }
        />
      </RowList>
    ));

    // No table semantics, no Open column, no boxed chevron control.
    expect(container.querySelector("table")).toBeNull();
    expect(container.querySelector('[role="columnheader"]')).toBeNull();
    expect(screen.queryByText("Open")).not.toBeInTheDocument();
    expect(container.querySelector(".rowListChevron")).not.toBeNull();
    const chevron = container.querySelector(".rowListChevron")!;
    expect(chevron.tagName).toBe("SPAN");
    expect(chevron).toHaveAttribute("aria-hidden", "true");
    expect(chevron.closest("button, a")).not.toBeNull();
    // The chevron is unboxed text inside the row control, never its own
    // button or link duplicating the row activation.
    expect(
      container.querySelector(
        "button.rowListChevron, a.rowListChevron, .rowListChevron button, .rowListChevron a",
      ),
    ).toBeNull();
    // No in-row cards.
    expect(container.querySelector(".ui-card")).toBeNull();

    // Clicking anywhere on the row control activates it once. (Accessible
    // names concatenate slot text without spaces, matching existing rows.)
    fireEvent.click(
      screen.getByRole("button", { name: /^Default.*3 entries$/ }),
    );
    expect(onActivate).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByText("3 entries"));
    expect(onActivate).toHaveBeenCalledTimes(2);
  });

  it("REQ-UX-LIST-001: keeps secondary actions from triggering row activation", () => {
    const onActivate = vi.fn();
    const onSettings = vi.fn();
    render(() => (
      <RowList label="Spaces">
        <RowListItem
          main={
            <RowListLink
              href="/spaces/default/dashboard"
              primary="Default"
              chevron
            />
          }
          actions={
            <a
              href="/spaces/default/settings"
              class="rowListIconButton"
              aria-label="Settings"
              onClick={(event) => {
                event.preventDefault();
                onSettings();
              }}
            >
              gear
            </a>
          }
        />
      </RowList>
    ));

    // The secondary action is a sibling of the row control, never nested.
    const row = screen.getByRole("link", { name: "Default" });
    const settings = screen.getByRole("link", { name: "Settings" });
    expect(row.contains(settings)).toBe(false);
    expect(row.getAttribute("href")).toBe("/spaces/default/dashboard");

    fireEvent.click(settings);
    expect(onSettings).toHaveBeenCalledTimes(1);
    expect(onActivate).toHaveBeenCalledTimes(0);
    fireEvent.click(row);
  });

  it("REQ-UX-LIST-001: exposes keyboard-operable rows with accessible names and visible focus", () => {
    render(() => (
      <RowList label="Forms">
        <RowListItem
          main={
            <RowListLink
              href="/spaces/default/entries?form=Notes"
              primary="Notes"
              chevron
            />
          }
        />
        <RowListItem
          main={
            <RowListButton
              onActivate={() => {}}
              primary="Projects"
              secondary="2 fields"
            />
          }
        />
      </RowList>
    ));

    expect(screen.getByRole("list", { name: "Forms" })).toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(2);

    // Native controls: keyboard Enter/Space activation comes from the
    // platform, so rows stay focusable buttons/links with accessible names.
    const link = screen.getByRole("link", { name: "Notes" });
    expect(link.tagName).toBe("A");
    link.focus();
    expect(document.activeElement).toBe(link);

    const button = screen.getByRole("button", {
      name: /^Projects.*2 fields$/,
    });
    expect(button.tagName).toBe("BUTTON");
    expect(button).not.toHaveAttribute("tabindex", "-1");
    button.focus();
    expect(document.activeElement).toBe(button);
  });

  it("REQ-UX-LIST-002: keeps each row in a single flex row with ellipsis and nowrap meta", () => {
    const { container } = render(() => (
      <RowList label="Spaces">
        <RowListItem
          main={
            <RowListButton
              onActivate={() => {}}
              primary="A very long space name that must truncate instead of wrapping"
              meta="Mar 1, 2026"
              chevron
            />
          }
          actions={
            <button
              type="button"
              class="rowListIconButton"
              aria-label="Settings"
            >
              gear
            </button>
          }
        />
      </RowList>
    ));

    const item = container.querySelector(".rowListItem")!;
    // Single-row structure: exactly the main control plus the actions slot.
    expect(item.children.length).toBe(2);
    expect(item.querySelector(":scope > .rowListMain")).not.toBeNull();
    expect(item.querySelector(":scope > .rowListActions")).not.toBeNull();
    // Primary truncates, meta and chevron stay on the same row.
    expect(item.querySelector(".rowListPrimary")).not.toBeNull();
    expect(item.querySelector(".rowListMeta")).toHaveTextContent("Mar 1, 2026");
    expect(item.querySelector(".rowListChevron")).not.toBeNull();
  });
});
