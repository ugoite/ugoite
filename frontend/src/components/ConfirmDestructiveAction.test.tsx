import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { ConfirmDestructiveAction } from "./ConfirmDestructiveAction";

function renderDialog(overrides: Partial<
  Parameters<typeof ConfirmDestructiveAction>[0]
> = {}) {
  const onConfirm = vi.fn();
  const onClose = vi.fn();
  const [open, setOpen] = createSignal(true);
  const result = render(() => (
    <div id="app">
      <button type="button" onClick={() => setOpen(true)}>
        Open delete
      </button>
      <ConfirmDestructiveAction
        open={open()}
        title="Delete this entry?"
        body="Are you sure you want to delete this entry?"
        confirmLabel="Delete entry"
        onConfirm={onConfirm}
        onClose={() => {
          onClose();
          setOpen(false);
        }}
        {...overrides}
      />
    </div>
  ));
  return { ...result, onConfirm, onClose, setOpen };
}

describe("ConfirmDestructiveAction", () => {
  beforeEach(() => {
    setLocale("en");
  });

  it("names the dialog and describes the consequence", () => {
    renderDialog();
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveAccessibleName("Delete this entry?");
    expect(dialog).toHaveAccessibleDescription(
      "Are you sure you want to delete this entry?",
    );
  });

  it("focuses the safe Cancel action by default, not the destructive one", async () => {
    renderDialog();
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus()
    );
    expect(
      screen.getByRole("button", { name: "Delete entry" }),
    ).not.toHaveFocus();
  });

  it("confirms only through the explicit confirm action", () => {
    const { onConfirm } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Delete entry" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("dismisses through Escape, Cancel, and the backdrop", async () => {
    const { onClose, setOpen } = renderDialog();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );

    setOpen(true);
    await screen.findByRole("dialog");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(2);
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );

    setOpen(true);
    await screen.findByRole("dialog");
    fireEvent.click(document.querySelector(".ui-backdrop")!);
    expect(onClose).toHaveBeenCalledTimes(3);
  });

  it("traps Tab inside the dialog", async () => {
    renderDialog();
    const dialog = screen.getByRole("dialog");
    const cancel = screen.getByRole("button", { name: "Cancel" });
    const confirm = screen.getByRole("button", { name: "Delete entry" });
    await waitFor(() => expect(cancel).toHaveFocus());

    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(confirm).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(cancel).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(confirm).toHaveFocus();
  });

  it("keeps the background inert while open and lifts it on dismiss", async () => {
    renderDialog();
    const app = document.getElementById("app")!;
    expect(app).toHaveAttribute("inert");
    expect(screen.getByRole("dialog").closest("#app")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    expect(app).not.toHaveAttribute("inert");
  });

  it("returns focus to the invoking control on dismiss", async () => {
    const { setOpen } = renderDialog();
    await screen.findByRole("dialog");
    // Dismiss, focus the opener, and reopen so the dialog records it.
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    const opener = screen.getByRole("button", { name: "Open delete" });
    opener.focus();
    setOpen(true);
    await screen.findByRole("dialog");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(opener).toHaveFocus());
  });

  it("holds dismiss while busy and surfaces dialog errors", () => {
    const { onClose } = renderDialog({
      busy: true,
      error: "Failed to delete the entry.",
    });
    const dialog = screen.getByRole("dialog");
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete entry" }))
      .toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Failed to delete the entry.",
    );
  });
});
