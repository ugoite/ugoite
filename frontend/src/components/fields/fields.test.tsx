import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FieldInput } from "~/components/fields/FieldInput";
import { FieldValues } from "~/components/fields/FieldValue";
import { RowReferenceSelect } from "~/components/fields/RowReferenceSelect";
import {
  buildRowReferenceOptions,
  hasRowReferencePicker,
} from "~/components/fields/row-reference";
import { entryApi } from "~/lib/entry-api";
import type { Entry } from "~/lib/types";

const projectForms = [{
  id: "form-project",
  name: "Project",
  version: 1,
  template: "",
  fields: {
    Title: { type: "string", required: false },
  },
}];

const projectEntry = (overrides?: Partial<Entry>): Entry => ({
  id: "entry-off-page",
  form: "Project",
  fields: { Title: "Off-page project" },
  revision_id: "rev-1",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-02T00:00:00Z",
  ...overrides,
});

afterEach(() => {
  vi.restoreAllMocks();
});
describe("shared FieldInput family", () => {
  it("edits strings and numbers with the same semantics in both call sites", () => {
    const onSummary = vi.fn();
    const onAmount = vi.fn();
    render(() => (
      <>
        <label for="create-summary">Summary</label>
        <FieldInput
          field={{ type: "string" }}
          value="hello"
          onChange={onSummary}
          fieldId="create-summary"
        />
        <label for="edit-amount">Amount</label>
        <FieldInput
          field={{ type: "double" }}
          value="12."
          onChange={onAmount}
          fieldId="edit-amount"
        />
      </>
    ));

    const summary = screen.getByLabelText("Summary");
    expect(summary).toHaveAttribute("type", "text");
    expect(summary).toHaveValue("hello");
    fireEvent.input(summary, { target: { value: "world" } });
    expect(onSummary).toHaveBeenCalledWith("world");

    // Number-oriented text preserves partial input for Rust to judge.
    const amount = screen.getByLabelText("Amount");
    expect(amount).toHaveAttribute("type", "text");
    expect(amount).toHaveAttribute("inputmode", "decimal");
    expect(amount).toHaveValue("12.");
  });

  it("renders booleans as a typed checkbox and keeps legacy text readable", () => {
    const onChecked = vi.fn();
    render(() => (
      <>
        <label for="bool-checked">Done</label>
        <FieldInput
          field={{ type: "boolean" }}
          value="yes"
          onChange={onChecked}
          fieldId="bool-checked"
        />
        <label for="bool-legacy">Legacy</label>
        <FieldInput
          field={{ type: "boolean" }}
          value="maybe"
          onChange={vi.fn()}
          fieldId="bool-legacy"
        />
      </>
    ));

    const box = screen.getByLabelText("Done");
    expect(box).toHaveAttribute("type", "checkbox");
    expect(box).toBeChecked();
    fireEvent.click(box);
    expect(onChecked).toHaveBeenCalledWith(false);

    // Unparseable legacy text stays a text control for Rust to diagnose.
    const legacy = screen.getByLabelText("Legacy");
    expect(legacy).toHaveAttribute("type", "text");
    expect(legacy).toHaveValue("maybe");
  });

  it("renders an unparseable boolean draft as editable text", () => {
    // Rust-owned validation must keep an unparseable legacy value visible
    // instead of coercing it into a checkbox state.
    render(() => (
      <FieldInput
        field={{ type: "boolean" }}
        value="maybe"
        onChange={vi.fn()}
        fieldId="bool-late"
      />
    ));

    expect(screen.getByRole("textbox")).toHaveAttribute("type", "text");
    expect(screen.getByRole("textbox")).toHaveValue("maybe");
  });

  it("renders dates with a date control", () => {
    render(() => (
      <>
        <label for="due-date">Due</label>
        <FieldInput
          field={{ type: "date" }}
          value="2026-02-14"
          onChange={vi.fn()}
          fieldId="due-date"
        />
      </>
    ));
    const due = screen.getByLabelText("Due");
    expect(due).toHaveAttribute("type", "date");
    expect(due).toHaveValue("2026-02-14");
  });

  it("shares the same object_list row UI for create and edit values", () => {
    const onChange = vi.fn();
    const [values, setValues] = createSignal<Record<string, unknown>[]>([
      { name: "alpha" },
    ]);
    render(() => (
      <FieldInput
        field={{ type: "object_list" }}
        value={values()}
        onChange={(next) => {
          setValues(next as Record<string, unknown>[]);
          onChange(next);
        }}
        fieldId="rows"
      />
    ));

    expect(screen.getByDisplayValue("alpha")).toBeInTheDocument();
    fireEvent.input(screen.getByDisplayValue("alpha"), {
      target: { value: "beta" },
    });
    expect(onChange).toHaveBeenCalledWith([{ name: "beta" }]);
  });
});

describe("shared RowReferenceSelect", () => {
  it("uses the target Form for the canonical picker without exposing ids", () => {
    const onChange = vi.fn();
    render(() => (
      <RowReferenceSelect
        spaceId="default"
        targetForm="Project"
        value=""
        onChange={onChange}
        fieldId="project-canonical"
        forms={[{
          id: "form-project",
          name: "Project",
          version: 1,
          template: "",
          fields: {},
        }]}
      />
    ));

    const picker = screen.getByTestId("row-reference-picker");
    expect(picker).toHaveAttribute("data-target-form", "form-project");
    expect(screen.getByRole("button", { name: /select entry/i }))
      .toBeInTheDocument();
    expect(screen.queryByText("entry-project-1")).not.toBeInTheDocument();
    expect(onChange).not.toHaveBeenCalled();
  });
  it("does not expose raw ids when the target Form catalog is unavailable", () => {
    render(() => (
      <RowReferenceSelect
        spaceId="default"
        targetForm="Project"
        value="stable-entry-id"
        onChange={vi.fn()}
        fieldId="project-unavailable"
      />
    ));

    expect(screen.getByRole("alert")).toHaveTextContent(
      "target Form Project is not available",
    );
    expect(screen.queryByText("stable-entry-id")).not.toBeInTheDocument();
  });
});

describe("RowReferenceSelect point hydration", () => {
  it("shows the Preview for a stored id outside the candidate page", async () => {
    const getEntry = vi.spyOn(entryApi, "get").mockResolvedValue(
      projectEntry(),
    );
    render(() => (
      <RowReferenceSelect
        spaceId="default"
        targetForm="Project"
        value="entry-off-page"
        onChange={vi.fn()}
        fieldId="project-point-read"
        forms={projectForms}
      />
    ));

    expect(await screen.findByText("Title: Off-page project"))
      .toBeInTheDocument();
    expect(getEntry).toHaveBeenCalledWith("default", "entry-off-page");
    expect(screen.queryByText("entry-off-page")).not.toBeInTheDocument();
  });

  it("shows a safe unavailable state for deleted references", async () => {
    vi.spyOn(entryApi, "get").mockRejectedValue(new Error("not found"));
    render(() => (
      <RowReferenceSelect
        spaceId="default"
        targetForm="Project"
        value="entry-deleted"
        onChange={vi.fn()}
        fieldId="project-deleted"
        forms={projectForms}
      />
    ));

    expect(await screen.findByText(/no longer have access/))
      .toBeInTheDocument();
    expect(screen.queryByText("entry-deleted")).not.toBeInTheDocument();
  });

  it("names the actual Form for wrong-Form references without raw ids", async () => {
    vi.spyOn(entryApi, "get").mockResolvedValue(
      projectEntry({ id: "entry-other", form: "Other" }),
    );
    render(() => (
      <RowReferenceSelect
        spaceId="default"
        targetForm="Project"
        value="entry-other"
        onChange={vi.fn()}
        fieldId="project-wrong-form"
        forms={projectForms}
      />
    ));

    expect(
      await screen.findByText(
        "The selected entry belongs to Other, not Project.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText("entry-other")).not.toBeInTheDocument();
  });
});

describe("RowReferenceSelect picker dialog", () => {
  const candidateRows = [{
    id: "project-alpha",
    form_id: "form-project",
    revision_id: "rev-1",
    created_at_micros: 1_000_000,
    updated_at_micros: 1_000_000,
    preview: "Project Alpha",
  }];

  const renderPicker = (props?: {
    value?: string;
    onChange?: (id: string) => void;
    onPendingChange?: (pending: boolean) => void;
  }) => {
    vi.spyOn(entryApi, "query").mockResolvedValue({
      rows: candidateRows,
      has_more: false,
    });
    const onChange = props?.onChange ?? vi.fn();
    const onPendingChange = props?.onPendingChange ?? vi.fn();
    render(() => (
      <RowReferenceSelect
        spaceId="default"
        targetForm="Project"
        value={props?.value ?? ""}
        onChange={onChange}
        fieldId="project-picker"
        forms={projectForms}
        onPendingChange={onPendingChange}
      />
    ));
    return { onChange, onPendingChange };
  };

  const openPicker = async () => {
    fireEvent.click(screen.getByRole("button", { name: "Select entry" }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(await screen.findByText("Project Alpha")).toBeInTheDocument();
  };

  const flushFocus = async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  };

  it("focuses the safe action first and returns focus to the trigger", async () => {
    renderPicker();
    const trigger = screen.getByRole("button", { name: "Select entry" });
    fireEvent.click(trigger);
    await screen.findByRole("dialog");
    await flushFocus();
    expect(document.activeElement?.textContent).toBe("Cancel");

    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    // Confirm stays disabled until a row is pending; dialog remains open.
    expect(screen.getByRole("dialog")).toBeInTheDocument();

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await flushFocus();
    expect(document.activeElement).toBe(trigger);
  });

  it("traps Tab inside the dialog", async () => {
    renderPicker();
    fireEvent.click(screen.getByRole("button", { name: "Select entry" }));
    const dialog = await screen.findByRole("dialog");
    await screen.findByText("Project Alpha");
    await flushFocus();

    // Confirm enables once a row is pending, so it can take focus.
    fireEvent.click(screen.getByRole("button", { name: "Use this entry" }));
    const confirm = screen.getByRole("button", { name: "Confirm" });
    expect(confirm).toBeEnabled();
    confirm.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(document.activeElement?.textContent).toBe("Cancel");

    (document.activeElement as HTMLElement).focus();
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(confirm);
  });

  it("commits the stable id only on confirm, never on row click", async () => {
    const { onChange, onPendingChange } = renderPicker();
    await openPicker();

    fireEvent.click(screen.getByRole("button", { name: "Use this entry" }));
    expect(onChange).not.toHaveBeenCalled();
    expect(onPendingChange).toHaveBeenCalledWith(true);
    expect(screen.getByRole("button", { name: "Confirm" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    expect(onChange).toHaveBeenCalledWith("project-alpha");
    expect(onPendingChange).toHaveBeenLastCalledWith(false);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("cancels without mutating the draft", async () => {
    const { onChange, onPendingChange } = renderPicker();
    await openPicker();

    fireEvent.click(screen.getByRole("button", { name: "Use this entry" }));
    fireEvent.click(
      screen.getAllByRole("button", { name: "Cancel" }).at(-1)!,
    );
    expect(onChange).not.toHaveBeenCalled();
    expect(onPendingChange).toHaveBeenLastCalledWith(false);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});

describe("list<row_reference> rows", () => {
  it("uses the same selector per row and stores stable ids", () => {
    const onChange = vi.fn();
    const [values] = createSignal<string[]>([
      "project-alpha",
      "project-beta",
    ]);
    render(() => (
      <FieldInput
        field={{
          type: "list",
          items: { type: "row_reference", target_form: "Project" },
        }}
        value={values()}
        onChange={onChange}
        fieldId="projects"
        spaceId="default"
        forms={[{
          id: "form-project",
          name: "Project",
          version: 1,
          template: "",
          fields: {},
        }]}
      />
    ));

    expect(screen.getAllByRole("button", { name: /select entry/i }))
      .toHaveLength(2);
  });
});

describe("row-reference helpers", () => {
  it("builds sorted human-readable options with stable ids", () => {
    expect(
      buildRowReferenceOptions([
        { id: "b" },
        { id: "a" },
        { id: "c" },
      ]),
    ).toEqual([
      { id: "a", title: "a", label: "a" },
      { id: "b", title: "b", label: "b" },
      { id: "c", title: "c", label: "c" },
    ]);
  });

  it("scopes the picker to the exact target form", () => {
    expect(
      hasRowReferencePicker(
        { type: "row_reference", target_form: "Project" },
        "default",
      ),
    ).toBe(true);
    expect(
      hasRowReferencePicker({ type: "row_reference" }, "default"),
    ).toBe(false);
    expect(
      hasRowReferencePicker(
        { type: "row_reference", target_form: "Project" },
        "",
      ),
    ).toBe(false);
  });
});

describe("read-only FieldValues", () => {
  it("renders values as text with no disabled inputs", () => {
    const { container } = render(() => (
      <FieldValues
        fields={[{ name: "Summary" }, { name: "Tags" }]}
        getValue={(name) => name === "Summary" ? "hello" : ["alpha", "beta"]}
      />
    ));
    expect(screen.getByText("hello")).toBeInTheDocument();
    expect(screen.getByText("alpha")).toBeInTheDocument();
    expect(container.querySelector("input, textarea, select")).toBeNull();
  });
});
