import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { compositionApi } from "~/lib/composition-api";
import type {
  CompositionLintDocument,
  CompositionResolvePlan,
  CompositionSourcePage,
} from "~/lib/composition-api";
import { setLocale } from "~/lib/i18n";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import CompositionRevisionRoute from "./index";

const navigateMock = vi.hoisted(() => vi.fn());

vi.mock("@solidjs/router", () => ({
  useParams: () => ({
    space_id: "space-1",
    composition_id: "tool-1",
    revision_id: "revision-2",
  }),
  useNavigate: () => navigateMock,
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    ["aria-label"]?: string;
    title?: string;
  }) => (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
      title={props.title}
    >
      {props.children as never}
    </a>
  ),
}));

const rawRevision = {
  revision: {
    entry_id: "tool-1",
    revision_id: "revision-2",
    committed_at_micros: 1,
  },
  fields: { name: "Monthly expenses" },
  unmapped_field_values: {},
};

const plan = {
  composition_revision: { entry_id: "tool-1", revision_id: "revision-2" },
  sources: [],
  component_bindings: [],
  dependencies: {
    variable_consumers: {
      month_start: {},
      include_archived: {},
      month: {},
      region: {},
    },
  },
};

describe("Composition exact-revision route", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    navigateMock.mockClear();
    setLocale("en");
    vi.spyOn(compositionApi, "get").mockResolvedValue(rawRevision);
    vi.spyOn(compositionApi, "resolve").mockResolvedValue({
      ok: true,
      variable_definitions: [{
        id: "month_start",
        type: "date",
        required: true,
        default: "2026-01-01",
        format: "year-month",
      }],
      plan,
    });
    vi.spyOn(compositionApi, "list").mockResolvedValue({
      items: [{
        composition_id: "tool-1",
        revision_id: "revision-2",
        updated_at: 1767312000,
        name: "Monthly expenses",
        kind: "dashboard",
      }],
      offset: 0,
      limit: 100,
      has_more: false,
    });
    vi.spyOn(compositionApi, "restore").mockResolvedValue({
      composition_id: "tool-1",
      revision_id: "revision-3",
      restored_from_revision_id: "revision-2",
      canonical_yaml: "canonical yaml",
      receipt: {
        command_id: "command-1",
        catalog_generation: 4,
        snapshot_id: 42,
        committed_revision_ids: ["revision-3"],
        committed_at_micros: 1,
        data_file_count: 1,
      },
    });
  });

  it("resolves typed Input changes against the exact revision", async () => {
    render(() => <CompositionRevisionRoute />);

    expect(await screen.findByRole("heading", { name: "Monthly expenses" }))
      .toBeInTheDocument();
    expect(compositionApi.get).toHaveBeenCalledWith(
      "space-1",
      "tool-1",
      "revision-2",
      expect.any(AbortSignal),
    );
    expect(compositionApi.resolve).toHaveBeenCalledWith(
      "space-1",
      "tool-1",
      "revision-2",
      {},
      expect.any(AbortSignal),
    );
    const input = await screen.findByLabelText("month_start");
    expect(input).toHaveValue("2026-01-01");
    expect(input).not.toHaveAttribute("readonly");
    expect(input).toHaveAttribute("aria-required", "true");
    fireEvent.input(input, { target: { value: "2026-02-01" } });
    await waitFor(() =>
      expect(compositionApi.resolve).toHaveBeenCalledTimes(2)
    );
    expect(compositionApi.resolve).toHaveBeenLastCalledWith(
      "space-1",
      "tool-1",
      "revision-2",
      { month_start: "2026-02-01" },
      expect.any(AbortSignal),
    );
  });

  it("owns the tool name once and links a single edit action", async () => {
    render(() => <CompositionRevisionRoute />);

    expect(await screen.findByRole("heading", { name: "Monthly expenses" }))
      .toBeInTheDocument();
    expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1);
    const edit = screen.getByRole("link", { name: "Edit" });
    expect(edit).toHaveAttribute(
      "href",
      "/spaces/space-1/compositions/tool-1/revision-2/edit",
    );
  });

  it("renders deterministic diagnostics and retains the Input control", async () => {
    vi.mocked(compositionApi.resolve).mockResolvedValue({
      ok: false,
      variable_definitions: [{
        id: "month_start",
        type: "date",
        required: true,
      }],
      diagnostics: [{
        code: "variable_missing",
        variable_id: "month_start",
      }],
    });
    render(() => <CompositionRevisionRoute />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Enter a value for month_start.",
    );
    expect(screen.getByLabelText("month_start")).toHaveAttribute(
      "aria-required",
      "true",
    );
    expect(screen.getByLabelText("month_start")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
  });

  it("preserves false as the default for a required boolean variable", async () => {
    vi.mocked(compositionApi.resolve).mockResolvedValue({
      ok: true,
      variable_definitions: [{
        id: "include_archived",
        type: "boolean",
        required: true,
        default: false,
      }],
      plan,
    });
    render(() => <CompositionRevisionRoute />);

    const input = await screen.findByLabelText("include_archived");
    expect(input).toHaveValue("false");
    expect(input).not.toHaveAttribute("readonly");
    expect(input).toHaveAttribute("aria-required", "true");
    expect(compositionApi.resolve).toHaveBeenCalledTimes(1);
    expect(compositionApi.resolve).toHaveBeenCalledWith(
      "space-1",
      "tool-1",
      "revision-2",
      {},
      expect.any(AbortSignal),
    );
  });

  it("retries an exact-revision open after a read failure", async () => {
    vi.mocked(compositionApi.get)
      .mockRejectedValueOnce(new Error("temporary read failure"))
      .mockResolvedValueOnce(rawRevision);
    render(() => <CompositionRevisionRoute />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not open this revision.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByRole("heading", { name: "Monthly expenses" }))
      .toBeInTheDocument();
    expect(compositionApi.get).toHaveBeenCalledTimes(2);
    expect(compositionApi.get).toHaveBeenLastCalledWith(
      "space-1",
      "tool-1",
      "revision-2",
      expect.any(AbortSignal),
    );
  });

  it("links to the composition history route", async () => {
    render(() => <CompositionRevisionRoute />);

    expect(await screen.findByRole("heading", { name: "Monthly expenses" }))
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: "History" })).toHaveAttribute(
      "href",
      "/spaces/space-1/compositions/tool-1/history",
    );
  });

  it("restores a historical revision with source base and a stable retry key", async () => {
    vi.mocked(compositionApi.list).mockResolvedValue({
      items: [{
        composition_id: "tool-1",
        revision_id: "revision-3",
        updated_at: 1767398400,
        name: "Monthly expenses",
      }],
      offset: 0,
      limit: 100,
      has_more: false,
    });
    vi.mocked(compositionApi.restore)
      .mockRejectedValueOnce(new Error("transport closed"))
      .mockResolvedValue({
        composition_id: "tool-1",
        revision_id: "revision-4",
        restored_from_revision_id: "revision-2",
        canonical_yaml: "canonical yaml",
        receipt: {
          command_id: "command-1",
          catalog_generation: 4,
          snapshot_id: 42,
          committed_revision_ids: ["revision-4"],
          committed_at_micros: 1,
          data_file_count: 1,
        },
      });
    render(() => <CompositionRevisionRoute />);

    // The opened revision is not the latest, so the single Restore action
    // shows next to the History link.
    const restore = await screen.findByRole("button", {
      name: "Restore this revision",
    });
    expect(screen.getByRole("link", { name: "History" })).toBeInTheDocument();

    fireEvent.click(restore);
    await waitFor(() => {
      expect(compositionApi.restore).toHaveBeenCalledTimes(1);
    });
    expect(compositionApi.restore).toHaveBeenCalledWith(
      "space-1",
      "tool-1",
      "revision-2",
      "revision-3",
      expect.any(String),
    );
    expect(await screen.findByRole("button", { name: "Retry" }))
      .toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => {
      expect(compositionApi.restore).toHaveBeenCalledTimes(2);
    });
    // The uncertain retry reuses the identical source, base, and key.
    expect(vi.mocked(compositionApi.restore).mock.calls[1]).toEqual(
      vi.mocked(compositionApi.restore).mock.calls[0],
    );
    expect(navigateMock).toHaveBeenCalledWith(
      "/spaces/space-1/compositions/tool-1/revision-4",
    );
  });

  it("hides restore on the latest revision", async () => {
    render(() => <CompositionRevisionRoute />);

    expect(await screen.findByRole("heading", { name: "Monthly expenses" }))
      .toBeInTheDocument();
    await waitFor(() =>
      expect(compositionApi.list).toHaveBeenCalledWith("space-1", 100, 0)
    );
    expect(
      screen.queryByRole("button", { name: "Restore this revision" }),
    ).not.toBeInTheDocument();
    expect(compositionApi.restore).not.toHaveBeenCalled();
  });

  it("surfaces a stale base conflict with a recovery link to history", async () => {
    vi.mocked(compositionApi.list).mockResolvedValue({
      items: [{
        composition_id: "tool-1",
        revision_id: "revision-3",
        updated_at: 1767398400,
        name: "Monthly expenses",
      }],
      offset: 0,
      limit: 100,
      has_more: false,
    });
    vi.mocked(compositionApi.restore).mockRejectedValueOnce(
      new UgoiteApiError({
        kind: "conflict",
        operation: "composition.restore",
        status: 409,
        message: "base revision is stale",
      }),
    );
    render(() => <CompositionRevisionRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Restore this revision" }),
    );

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Someone else saved first.",
    );
    const historyLinks = screen.getAllByRole("link", { name: "History" });
    expect(historyLinks.length).toBeGreaterThanOrEqual(1);
    for (const link of historyLinks) {
      expect(link).toHaveAttribute(
        "href",
        "/spaces/space-1/compositions/tool-1/history",
      );
    }
    expect(
      screen.queryByRole("button", { name: "Retry" }),
    ).not.toBeInTheDocument();
    expect(navigateMock).not.toHaveBeenCalled();
  });
});

describe("Composition revision flow layout", () => {
  const lintDocument: CompositionLintDocument = {
    format: "ugoite.composition",
    format_version: 1,
    kind: "dashboard",
    name: "Monthly expenses",
    spec: {
      variables: [
        {
          id: "month",
          type: "date",
          required: true,
          default: "2026-10-01",
          format: "year-month",
        },
        { id: "region", type: "string", required: false },
      ],
      sources: [{
        kind: "inline_sql",
        id: "entries",
        name: "Entries",
        sql: "SELECT purpose FROM entries",
        form_bindings: [],
        expected_result: [{ name: "purpose", type: "string" }],
        schema_fingerprint: "fingerprint",
      }],
      components: [
        { kind: "input", id: "input-month", variable: "month" },
        {
          kind: "text",
          id: "summary_title",
          content: { kind: "literal", text: "Monthly summary" },
          style: "heading",
        },
        { kind: "table", id: "rows", source: "entries" },
      ],
      layout: {
        kind: "flow",
        rows: [
          {
            id: "controls",
            items: [{ kind: "component", component: "input-month" }],
          },
          {
            id: "heading",
            items: [{ kind: "component", component: "summary_title" }],
          },
          { id: "detail", items: [{ kind: "component", component: "rows" }] },
        ],
      },
    },
  };

  const flowPlan: CompositionResolvePlan = {
    composition_revision: { entry_id: "tool-1", revision_id: "revision-2" },
    sources: [{
      kind: "inline_sql",
      source_id: "entries",
      source_schema_fingerprint: "fingerprint",
      request: { sql: "SELECT purpose FROM entries", limit: 100 },
      form_bindings: [],
      expected_result: [{ name: "purpose", type: "string" }],
    }],
    component_bindings: [
      {
        component_id: "input-month",
        kind: "input",
        variable_id: "month",
      },
      { component_id: "summary_title", kind: "text" },
      {
        component_id: "rows",
        kind: "table",
        source_id: "entries",
      },
    ],
    dependencies: {
      variable_consumers: {
        month: { source_ids: [] },
        region: { source_ids: [] },
      },
    },
  };

  const entryPage: CompositionSourcePage = {
    kind: "sql",
    page: {
      columns: ["purpose"],
      rows: [["Travel"]],
      has_more: false,
      result_schema: [{ name: "purpose", type: "string" }],
    },
  };

  beforeEach(() => {
    vi.restoreAllMocks();
    navigateMock.mockClear();
    setLocale("en");
    vi.spyOn(compositionApi, "get").mockResolvedValue({
      ...rawRevision,
      fields: { name: "Monthly expenses", spec: "format: ugoite.composition" },
    });
    vi.spyOn(compositionApi, "lint").mockResolvedValue({
      ok: true,
      value: {
        document: lintDocument,
        canonical_yaml: "canonical",
        fingerprint: "fingerprint",
      },
    });
    vi.spyOn(compositionApi, "resolve").mockResolvedValue({
      ok: true,
      variable_definitions: [
        {
          id: "month",
          type: "date",
          required: true,
          default: "2026-10-01",
          format: "year-month",
        },
        { id: "region", type: "string", required: false },
      ],
      plan: flowPlan,
    });
    vi.spyOn(compositionApi, "querySource").mockResolvedValue(entryPage);
    vi.spyOn(compositionApi, "list").mockResolvedValue({
      items: [{
        composition_id: "tool-1",
        revision_id: "revision-2",
        updated_at: 1767312000,
        name: "Monthly expenses",
        kind: "dashboard",
      }],
      offset: 0,
      limit: 100,
      has_more: false,
    });
  });

  it("renders placed controls in flow order with text and the source-native table", async () => {
    const { container } = render(() => <CompositionRevisionRoute />);

    expect(await screen.findByRole("heading", { name: "Monthly expenses" }))
      .toBeInTheDocument();
    // The stable variable ID is the control name and follows it into the flow.
    const month = await screen.findByLabelText("month");
    expect(month).toHaveValue("2026-10-01");
    // Rows render top to bottom: controls, heading text, then the table.
    const rows = await screen.findAllByText(/month|Monthly summary|purpose/);
    expect(rows.length).toBeGreaterThan(0);
    const flowRows = container.querySelectorAll(".compositionFlowRow");
    expect(flowRows).toHaveLength(3);
    expect(flowRows[0].textContent).toContain("month");
    expect(flowRows[1].textContent).toContain("Monthly summary");
    expect(flowRows[2].textContent).toContain("purpose");
    expect(screen.getByRole("table", { name: "Result pages" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Result pages" })).toHaveAttribute(
      "tabindex",
      "0",
    );
    expect(screen.getByRole("cell", { name: "Travel" })).toBeInTheDocument();
  });

  it("updates variable-backed Text immediately when a placed Input changes", async () => {
    const dynamicDocument = structuredClone(lintDocument);
    dynamicDocument.spec.variables.push({
      id: "title",
      type: "string",
      required: true,
      default: "Initial title",
    });
    dynamicDocument.spec.components.push({
      kind: "input",
      id: "input-title",
      variable: "title",
    }, {
      kind: "text",
      id: "dynamic_title",
      content: { kind: "variable", variable: "title" },
      style: "heading",
    });
    if (dynamicDocument.spec.layout?.kind !== "flow") {
      throw new Error("expected a flow layout");
    }
    const controls = dynamicDocument.spec.layout.rows.find((row) =>
      row.id === "controls"
    );
    if (!controls) throw new Error("expected the controls row");
    controls.items.push({ kind: "component", component: "input-title" });
    dynamicDocument.spec.layout.rows.push({
      id: "dynamic-title",
      items: [{ kind: "component", component: "dynamic_title" }],
    });
    vi.mocked(compositionApi.lint).mockResolvedValue({
      ok: true,
      value: {
        document: dynamicDocument,
        canonical_yaml: "canonical",
        fingerprint: "fingerprint",
      },
    });
    const dynamicPlan = structuredClone(flowPlan);
    dynamicPlan.component_bindings.push({
      component_id: "input-title",
      kind: "input",
      variable_id: "title",
    }, {
      component_id: "dynamic_title",
      kind: "text",
    });
    dynamicPlan.dependencies = {
      variable_consumers: {
        month: { source_ids: [] },
        region: { source_ids: [] },
        title: {
          input_component_ids: ["input-title"],
          text_component_ids: ["dynamic_title"],
          source_ids: [],
        },
      },
    };
    vi.mocked(compositionApi.resolve).mockResolvedValue({
      ok: true,
      variable_definitions: [{
        id: "month",
        type: "date",
        required: true,
        default: "2026-10-01",
        format: "year-month",
      }, {
        id: "region",
        type: "string",
        required: false,
      }, {
        id: "title",
        type: "string",
        required: true,
        default: "Initial title",
      }],
      plan: dynamicPlan,
    });
    render(() => <CompositionRevisionRoute />);

    expect(await screen.findByRole("heading", { name: "Initial title" }))
      .toBeInTheDocument();
    expect(compositionApi.querySource).toHaveBeenCalledOnce();
    fireEvent.input(screen.getByLabelText("title"), {
      target: { value: "Changed title" },
    });

    expect(screen.getByRole("heading", { name: "Changed title" }))
      .toBeInTheDocument();
    expect(compositionApi.resolve).toHaveBeenCalledOnce();
    await waitFor(() =>
      expect(compositionApi.resolve).toHaveBeenCalledTimes(2)
    );
    expect(compositionApi.querySource).toHaveBeenCalledOnce();
  });

  it("synchronizes repeated placed Inputs for valid and invalid raw values", async () => {
    const repeatedInputDocument = structuredClone(lintDocument);
    repeatedInputDocument.spec.variables.push({
      id: "minimum",
      type: "integer",
      required: true,
      default: 0,
    });
    repeatedInputDocument.spec.components.push({
      kind: "input",
      id: "input-minimum",
      variable: "minimum",
    }, {
      kind: "input",
      id: "input-minimum-copy",
      variable: "minimum",
    });
    if (repeatedInputDocument.spec.layout?.kind !== "flow") {
      throw new Error("expected a flow layout");
    }
    const controls = repeatedInputDocument.spec.layout.rows.find((row) =>
      row.id === "controls"
    );
    if (!controls) throw new Error("expected the controls row");
    controls.items.push(
      { kind: "component", component: "input-minimum" },
      { kind: "component", component: "input-minimum-copy" },
    );
    vi.mocked(compositionApi.lint).mockResolvedValue({
      ok: true,
      value: {
        document: repeatedInputDocument,
        canonical_yaml: "canonical",
        fingerprint: "fingerprint",
      },
    });
    const repeatedInputPlan = structuredClone(flowPlan);
    repeatedInputPlan.component_bindings.push({
      component_id: "input-minimum",
      kind: "input",
      variable_id: "minimum",
    }, {
      component_id: "input-minimum-copy",
      kind: "input",
      variable_id: "minimum",
    });
    repeatedInputPlan.dependencies = {
      variable_consumers: {
        month: { source_ids: [] },
        region: { source_ids: [] },
        minimum: {
          input_component_ids: ["input-minimum", "input-minimum-copy"],
          source_ids: [],
        },
      },
    };
    vi.mocked(compositionApi.resolve).mockResolvedValue({
      ok: true,
      variable_definitions: [{
        id: "month",
        type: "date",
        required: true,
        default: "2026-10-01",
        format: "year-month",
      }, {
        id: "region",
        type: "string",
        required: false,
      }, {
        id: "minimum",
        type: "integer",
        required: true,
        default: 0,
      }],
      plan: repeatedInputPlan,
    });
    render(() => <CompositionRevisionRoute />);

    const minimumInputs = await screen.findAllByLabelText("minimum");
    const minimum = minimumInputs[0];
    const minimumMirror = minimumInputs[1];
    expect(minimum).toHaveValue("0");
    expect(minimumMirror).toHaveValue("0");
    expect(compositionApi.querySource).toHaveBeenCalledOnce();

    fireEvent.input(minimum, { target: { value: "+00042" } });
    expect(minimum).toHaveValue("+00042");
    expect(minimumMirror).toHaveValue("+00042");
    await waitFor(() =>
      expect(compositionApi.resolve).toHaveBeenCalledTimes(2)
    );
    expect(compositionApi.querySource).toHaveBeenCalledOnce();

    fireEvent.input(minimumMirror, { target: { value: "1.0" } });
    expect(minimum).toHaveValue("1.0");
    expect(minimumMirror).toHaveValue("1.0");
    expect(minimum).toHaveAttribute("aria-invalid", "true");
    expect(minimumMirror).toHaveAttribute("aria-invalid", "true");
    expect(compositionApi.resolve).toHaveBeenCalledTimes(2);
    expect(compositionApi.querySource).toHaveBeenCalledOnce();
  });

  it("uses stable variable IDs when placement and declaration order differ", async () => {
    const variables = lintDocument.spec.variables ?? [];
    const layout = lintDocument.spec.layout;
    const controls = layout.rows.find((row) => row.id === "controls");
    const regionInput = {
      kind: "input" as const,
      id: "input-region",
      variable: "region",
    };
    const reorderedLayout = {
      ...layout,
      rows: [
        {
          ...controls!,
          items: [{ kind: "component" as const, component: "input-region" }],
        },
        ...layout.rows.filter((row) => row.id !== "controls"),
      ],
    };
    vi.mocked(compositionApi.lint).mockResolvedValueOnce({
      ok: true,
      value: {
        document: {
          ...lintDocument,
          spec: {
            ...lintDocument.spec,
            variables,
            components: [...lintDocument.spec.components, regionInput],
            layout: reorderedLayout,
          },
        },
        canonical_yaml: "canonical",
        fingerprint: "fingerprint",
      },
    });
    vi.mocked(compositionApi.resolve).mockResolvedValueOnce({
      ok: true,
      variable_definitions: [...variables].reverse(),
      plan: flowPlan,
    });
    const { container } = render(() => <CompositionRevisionRoute />);

    expect(await screen.findByLabelText("month")).toHaveValue(
      "2026-10-01",
    );
    expect(await screen.findByLabelText("region")).toBeInTheDocument();
    expect(container.querySelector(".compositionFlowRow")).toHaveTextContent(
      "region",
    );
    expect(container.querySelector(".compositionFlowRow")).not
      .toHaveTextContent("month");

    setLocale("ja");
    expect(screen.getByLabelText("month")).toHaveValue("2026-10-01");
    expect(screen.getByLabelText("region")).toBeInTheDocument();
  });

  it("shows unplaced variables outside the flow without duplication", async () => {
    render(() => <CompositionRevisionRoute />);

    expect(await screen.findByLabelText("region")).toBeInTheDocument();
    expect(screen.getByLabelText("region")).not.toHaveAttribute("readonly");
    // The placed control renders once, inside the flow only.
    expect(screen.getAllByLabelText("month")).toHaveLength(1);
    expect(screen.getByLabelText("month")).not.toHaveAttribute("readonly");
    expect(screen.getAllByLabelText("region")).toHaveLength(1);
  });

  it("re-resolves the exact revision when a placed Input changes", async () => {
    render(() => <CompositionRevisionRoute />);

    const month = await screen.findByLabelText("month");
    fireEvent.input(month, { target: { value: "2026-02-01" } });

    await waitFor(() =>
      expect(compositionApi.resolve).toHaveBeenCalledTimes(2)
    );
    expect(compositionApi.resolve).toHaveBeenLastCalledWith(
      "space-1",
      "tool-1",
      "revision-2",
      { month: "2026-02-01", region: null },
      expect.any(AbortSignal),
    );
  });

  it("falls back to the flat renderer when the layout cannot be normalized", async () => {
    vi.mocked(compositionApi.lint).mockRejectedValueOnce(
      new Error("temporary lint failure"),
    );
    render(() => <CompositionRevisionRoute />);

    expect(await screen.findByLabelText("month")).toBeInTheDocument();
    expect(screen.getByLabelText("region")).toBeInTheDocument();
    expect(screen.getByRole("table")).toBeInTheDocument();
    expect(document.querySelector(".compositionFlow")).toBeNull();
  });
});
