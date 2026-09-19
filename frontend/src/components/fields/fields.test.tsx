import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FieldInput } from "~/components/fields/FieldInput";
import { FieldValues } from "~/components/fields/FieldValue";
import { RowReferenceSelect } from "~/components/fields/RowReferenceSelect";
import {
  buildRowReferenceOptions,
  hasRowReferencePicker,
} from "~/components/fields/row-reference";
import { searchApi } from "~/lib/ugoite-client";

vi.mock("~/lib/ugoite-client", () => ({
  searchApi: { rowReferenceOptions: vi.fn() },
  entryApi: {},
}));

const rowOptionsMock = vi.mocked(searchApi.rowReferenceOptions);

const alphaEntries = [
  { id: "project-alpha", title: "Alpha Project" },
  { id: "project-beta", title: "Beta Project" },
];

beforeEach(() => {
  vi.resetAllMocks();
  rowOptionsMock.mockImplementation(
    async (
      _spaceId: string,
      _form: string,
      query: string,
    ): Promise<Array<{ id: string; title: string }>> =>
      alphaEntries.filter((entry) =>
        entry.title.toLowerCase().includes(query.toLowerCase()) ||
        entry.id.includes(query)
      ),
  );
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

  it("settles booleans on the text control when the draft arrives after mount", async () => {
    // Drafts load asynchronously: the value is undefined at mount and the
    // legacy text lands later. The control kind must follow reactively
    // instead of sticking with the mount-time checkbox.
    const [value, setValue] = createSignal<unknown>(undefined);
    render(() => (
      <>
        <label for="bool-late">Done</label>
        <FieldInput
          field={{ type: "boolean" }}
          value={value()}
          onChange={vi.fn()}
          fieldId="bool-late"
        />
      </>
    ));

    expect(screen.getByLabelText("Done")).toHaveAttribute(
      "type",
      "checkbox",
    );
    setValue("maybe");
    await waitFor(() => {
      expect(screen.getByLabelText("Done")).toHaveAttribute("type", "text");
    });
    expect(screen.getByLabelText("Done")).toHaveValue("maybe");
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

  it("shares the same object_list row UI for create and edit values", async () => {
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
  it("shows titles while saving the stable entry id", async () => {
    const onChange = vi.fn();
    render(() => (
      <>
        <label for="project">Project</label>
        <RowReferenceSelect
          spaceId="default"
          targetForm="Project"
          value=""
          onChange={onChange}
          fieldId="project"
        />
      </>
    ));

    fireEvent.input(screen.getByLabelText("Project"), {
      target: { value: "alpha" },
    });
    fireEvent.click(
      await screen.findByRole("button", { name: /Alpha Project/ }),
    );
    expect(onChange).toHaveBeenCalledWith("project-alpha");
    expect(await screen.findByText("project-alpha")).toBeInTheDocument();
    expect(rowOptionsMock).toHaveBeenCalledWith(
      "default",
      "Project",
      "alpha",
      8,
    );
  });

  it("supports keyboard arrows, Enter, Escape, and clear", async () => {
    const onChange = vi.fn();
    const [value, setValue] = createSignal("");
    render(() => (
      <>
        <label for="project-kb">Project</label>
        <RowReferenceSelect
          spaceId="default"
          targetForm="Project"
          value={value()}
          onChange={(next) => {
            setValue(next);
            onChange(next);
          }}
          fieldId="project-kb"
        />
      </>
    ));

    const input = screen.getByLabelText("Project");
    fireEvent.input(input, { target: { value: "project" } });
    await screen.findByRole("button", { name: /Alpha Project/ });

    // ArrowDown highlights the second option; Enter confirms its stable id.
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith("project-beta");

    // Escape cancels the in-progress search and reverts to the confirmed pick.
    onChange.mockClear();
    fireEvent.input(input, { target: { value: "zzz" } });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(input).toHaveValue("Beta Project");
    expect(onChange).toHaveBeenCalledWith("project-beta");

    // Clear removes the saved entry id.
    onChange.mockClear();
    fireEvent.click(
      await screen.findByRole("button", { name: "Clear selection" }),
    );
    expect(onChange).toHaveBeenCalledWith("");
  });

  it("renders loading, error, and empty states", async () => {
    let resolveOptions!: (
      value: Array<{ id: string; title: string }>,
    ) => void;
    rowOptionsMock.mockReturnValue(
      new Promise((resolve) => {
        resolveOptions = resolve;
      }),
    );
    render(() => (
      <>
        <label for="project-states">Project</label>
        <RowReferenceSelect
          spaceId="default"
          targetForm="Project"
          value=""
          onChange={vi.fn()}
          fieldId="project-states"
        />
      </>
    ));

    fireEvent.input(screen.getByLabelText("Project"), {
      target: { value: "alpha" },
    });
    expect(await screen.findByText(/Loading Project entries/))
      .toBeInTheDocument();
    resolveOptions([]);
    expect(
      await screen.findByText(/No Project entries matched/),
    ).toBeInTheDocument();
  });

  it("surfaces lookup failures without losing the control", async () => {
    rowOptionsMock.mockRejectedValue(new Error("offline"));
    render(() => (
      <>
        <label for="project-error">Project</label>
        <RowReferenceSelect
          spaceId="default"
          targetForm="Project"
          value=""
          onChange={vi.fn()}
          fieldId="project-error"
        />
      </>
    ));

    fireEvent.input(screen.getByLabelText("Project"), {
      target: { value: "alpha" },
    });
    expect(await screen.findByText(/Couldn't load Project entries/))
      .toBeInTheDocument();
    expect(screen.getByLabelText("Project")).toBeInTheDocument();
  });

  it("reports unresolved searches so required guards keep working", async () => {
    const onPending = vi.fn();
    render(() => (
      <>
        <label for="project-pending">Project</label>
        <RowReferenceSelect
          spaceId="default"
          targetForm="Project"
          value=""
          onChange={vi.fn()}
          fieldId="project-pending"
          onPendingChange={onPending}
        />
      </>
    ));

    fireEvent.input(screen.getByLabelText("Project"), {
      target: { value: "alpha" },
    });
    await waitFor(() => expect(onPending).toHaveBeenCalledWith(true));
    fireEvent.click(
      await screen.findByRole("button", { name: /Alpha Project/ }),
    );
    await waitFor(() => expect(onPending).toHaveBeenCalledWith(false));
  });
});

describe("list<row_reference> rows", () => {
  it("uses the same selector per row and stores stable ids", async () => {
    const onChange = vi.fn();
    const [values, setValues] = createSignal<string[]>(["project-alpha"]);
    render(() => (
      <FieldInput
        field={{
          type: "list",
          items: { type: "row_reference", target_form: "Project" },
        }}
        value={values()}
        onChange={(next) => {
          setValues(next as string[]);
          onChange(next);
        }}
        fieldId="projects"
        spaceId="default"
      />
    ));

    // Stored ids resolve to titles once options arrive.
    expect(await screen.findByDisplayValue("Alpha Project"))
      .toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Add/ }));
    const inputs = screen.getAllByRole("combobox");
    expect(inputs).toHaveLength(2);
    fireEvent.input(inputs[1], { target: { value: "beta" } });
    fireEvent.click(
      await screen.findByRole("button", { name: /Beta Project/ }),
    );
    expect(onChange).toHaveBeenCalledWith(["project-alpha", "project-beta"]);
  });
});

describe("row-reference helpers", () => {
  it("builds sorted human-readable options with stable ids", () => {
    expect(
      buildRowReferenceOptions([
        { id: "b", title: "Same" },
        { id: "a", title: "Same" },
        { id: "c", title: null },
      ]),
    ).toEqual([
      { id: "c", title: "c", label: "c" },
      { id: "a", title: "Same", label: "Same (a)" },
      { id: "b", title: "Same", label: "Same (b)" },
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
