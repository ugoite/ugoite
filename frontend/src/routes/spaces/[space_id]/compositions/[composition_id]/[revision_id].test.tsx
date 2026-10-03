import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { compositionApi } from "~/lib/composition-api";
import { setLocale } from "~/lib/i18n";
import CompositionRevisionRoute from "./[revision_id]";

vi.mock("@solidjs/router", () => ({
  useParams: () => ({
    space_id: "space-1",
    composition_id: "tool-1",
    revision_id: "revision-2",
  }),
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
});
