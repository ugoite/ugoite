import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { compositionApi } from "~/lib/composition-api";
import { setLocale } from "~/lib/i18n";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import CompositionRevisionRoute, { resolveCompositionFieldName } from "./index";
import type { Form } from "~/lib/types";

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
};

describe("Composition exact-revision route", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    navigateMock.mockClear();
    setLocale("en");
    vi.spyOn(compositionApi, "get").mockResolvedValue(rawRevision);
    vi.spyOn(compositionApi, "resolve").mockResolvedValue({
      ok: true,
      parameter_definitions: [{
        id: "month_start",
        label: "Month",
        type: "date",
        required: true,
        default: "2026-01-01",
        format: "year-month",
      }],
      plan,
    });
    vi.spyOn(compositionApi, "querySource").mockResolvedValue({
      kind: "entry_query",
      page: { rows: [], has_more: false },
    });
    vi.spyOn(compositionApi, "list").mockResolvedValue({
      items: [{
        composition_id: "tool-1",
        revision_id: "revision-2",
        updated_at: 1767312000,
        name: "Monthly expenses",
        kind: "dashboard",
        tags: [],
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

  it("loads and resolves only the exact revision and binds typed parameter changes", async () => {
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
    const input = await screen.findByLabelText("Month");
    expect(input).toHaveValue("2026-01-01");
    expect(input).toBeRequired();
    fireEvent.change(input, { target: { value: "2026-02-01" } });

    await waitFor(() =>
      expect(compositionApi.resolve).toHaveBeenLastCalledWith(
        "space-1",
        "tool-1",
        "revision-2",
        { month_start: "2026-02-01" },
        expect.any(AbortSignal),
      )
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

  it("renders deterministic diagnostics and retains the parameter control", async () => {
    vi.mocked(compositionApi.resolve).mockResolvedValue({
      ok: false,
      parameter_definitions: [{
        id: "month_start",
        label: "Month",
        type: "date",
        required: true,
      }],
      diagnostics: [{
        code: "parameter_missing",
        parameter_id: "month_start",
      }],
    });
    render(() => <CompositionRevisionRoute />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Enter a value for Month.",
    );
    expect(screen.getByLabelText("Month")).toBeRequired();
    expect(screen.getByLabelText("Month")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
  });

  it("accepts false as a supplied value for a required boolean parameter", async () => {
    vi.mocked(compositionApi.resolve).mockResolvedValue({
      ok: true,
      parameter_definitions: [{
        id: "include_archived",
        label: "Include archived",
        type: "boolean",
        required: true,
      }],
      plan,
    });
    render(() => <CompositionRevisionRoute />);

    const select = await screen.findByLabelText("Include archived");
    expect(select).toBeRequired();
    expect(select).toHaveValue("");
    fireEvent.change(select, { target: { value: "false" } });

    await waitFor(() =>
      expect(compositionApi.resolve).toHaveBeenLastCalledWith(
        "space-1",
        "tool-1",
        "revision-2",
        { include_archived: false },
        expect.any(AbortSignal),
      )
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
        tags: [],
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
        tags: [],
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

describe("resolveCompositionFieldName", () => {
  const forms = (): Form[] => [
    {
      id: "form-1",
      name: "Expenses",
      version: 1,
      template: "entry",
      fields: {
        amount: { id: 101, type: "number", required: true },
      },
    },
  ];

  it("resolves a field label by stable form id", () => {
    expect(resolveCompositionFieldName(forms(), "form-1", 101)).toBe("amount");
  });

  it("does not resolve by display name", () => {
    expect(resolveCompositionFieldName(forms(), "Expenses", 101))
      .toBeUndefined();
  });

  it("is rename-stable and display-only", () => {
    const renamed = forms();
    renamed[0].name = "Renamed expenses";
    // Display metadata renames never change id-based resolution.
    expect(resolveCompositionFieldName(renamed, "form-1", 101)).toBe("amount");
    expect(resolveCompositionFieldName(renamed, "Renamed expenses", 101))
      .toBeUndefined();
    // Unknown forms and fields stay unresolved so the table falls back
    // to the current row key order; the lookup never mutates its input.
    expect(resolveCompositionFieldName(renamed, "missing-form", 101))
      .toBeUndefined();
    expect(resolveCompositionFieldName(renamed, "form-1", 999)).toBeUndefined();
    expect(renamed).toEqual(
      forms().map((form) =>
        form.id === "form-1" ? { ...form, name: "Renamed expenses" } : form
      ),
    );
  });
});
