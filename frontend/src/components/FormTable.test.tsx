import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import {
  chunkCsvDataRowsForExport,
  chunkCsvRowsForExport,
  encodeSpreadsheetCsvChunked,
  encodeSpreadsheetCsvDataRowsChunked,
  FormTable,
} from "./FormTable";
import {
  encodeSpreadsheetCsv,
  entryApi,
  spreadsheetCsvRequestBytes,
} from "~/lib/ugoite-client";
import type { Form } from "~/lib/types";
import type { EntryPage } from "~/lib/entry-query";
import { setLocale } from "~/lib/i18n";

type QueryTestEntry = {
  id: string;
  properties?: Record<string, unknown>;
  created_at?: string;
  updated_at?: string;
};

function entryQueryPage(entries: readonly QueryTestEntry[]): EntryPage {
  const toMicros = (value: string | undefined) => {
    const parsed = Date.parse(value ?? "");
    return Number.isFinite(parsed) ? parsed * 1_000 : 0;
  };
  return {
    rows: entries.map((entry, index) => ({
      id: entry.id,
      form_id: "form-test",
      revision_id: `revision-${index}`,
      created_at_micros: toMicros(entry.created_at),
      updated_at_micros: toMicros(entry.updated_at),
      properties: entry.properties,
    })),
    has_more: false,
  };
}

function mockEntryQuery(entries: readonly QueryTestEntry[]) {
  return vi.spyOn(entryApi, "query").mockResolvedValue(entryQueryPage(entries));
}

const canonicalForms = new WeakMap<object, Form>();

function canonicalForm(form: Record<string, any>): Form {
  const cached = canonicalForms.get(form);
  if (cached) return cached;
  const canonical: Form = {
    ...form,
    id: form.id ?? `form-${form.name}`,
    version: form.version ?? 1,
    template: form.template ?? "",
    fields: Object.fromEntries(
      Object.entries(form.fields ?? {}).map(([name, rawField], index) => {
        const field = rawField as Record<string, any>;
        const fieldId = field.id ?? index + 1;
        return [name, {
          ...field,
          type: field.type ?? "string",
          required: field.required ?? false,
          id: fieldId,
          query_capability: field.query_capability ?? {
            field: { kind: "property", field_id: fieldId },
            name,
            field_type: field.type === "double" ? "numeric" : "string",
            filterable: true,
            sortable: true,
            projectable: true,
            supported_operators: field.type === "double"
              ? ["equals", "lt", "lte", "gt", "gte"]
              : ["equals", "contains"],
          },
        }] as const;
      }),
    ),
  };
  canonicalForms.set(form, canonical);
  return canonical;
}

function desktopTable() {
  const table = document.querySelector(".ui-table-desktop");
  if (!table) throw new Error("desktop table not found");
  return within(table as HTMLElement);
}

function mobileList() {
  const list = document.querySelector(".ui-table-mobile-list");
  if (!list) throw new Error("mobile list not found");
  return within(list as HTMLElement);
}

describe("FormTable", () => {
  beforeEach(() => {
    setLocale("en");
    vi.restoreAllMocks();
  });

  it("keeps the Form workspace available when its query fails", async () => {
    const entryForm = { name: "Entry", fields: {} } as any;
    const query = vi.spyOn(entryApi, "query")
      .mockRejectedValueOnce(new Error("Internal server error"))
      .mockResolvedValue(entryQueryPage([]));

    const { getByRole, getByText, queryByRole } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() =>
      expect(getByRole("alert")).toHaveTextContent(
        "Could not load records for this Form.",
      )
    );

    fireEvent.click(getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(query).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(queryByRole("alert")).not.toBeInTheDocument());
    expect(getByText("0 records found")).toBeInTheDocument();
  });

  it("renders '-' for missing properties and does not throw", async () => {
    const entryForm = {
      name: "Test",
      fields: { A: { type: "string" }, B: { type: "string" } },
    } as any;
    const entries = [
      {
        id: "1",
        properties: undefined,
        updated_at: new Date().toISOString(),
      },
    ];

    const spy = mockEntryQuery(entries);

    render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() => {
      expect(spy).toHaveBeenCalled();
      const matches = document.querySelectorAll("td");
      // Ensure at least one cell contains the placeholder
      const hyphens = Array.from(matches).filter((n) => n.textContent === "-");
      expect(hyphens.length).toBeGreaterThan(0);
    });

    spy.mockRestore();
  });

  it("uses a form-scoped keyset page chain", async () => {
    const entryForm = {
      name: "Test",
      fields: { status: { type: "string" } },
    } as any;
    const pages = new Map<string | undefined, EntryPage>([
      [undefined, {
        rows: [{
          id: "entry-1",
          form_id: "form-Test",
          revision_id: "revision-1",
          created_at_micros: 1,
          updated_at_micros: 2,
          properties: { status: "open" },
        }],
        has_more: true,
        next: "cursor-1",
      }],
      ["cursor-1", {
        rows: [{
          id: "entry-2",
          form_id: "form-Test",
          revision_id: "revision-2",
          created_at_micros: 3,
          updated_at_micros: 4,
          properties: { status: "closed" },
        }],
        has_more: false,
      }],
    ]);
    const query = vi.spyOn(entryApi, "query").mockImplementation(
      async (_spaceId, request) => pages.get(request.after)!,
    );

    const { getByRole } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
      />
    ));

    await waitFor(() =>
      expect(desktopTable().getByText("entry-1"))
        .toBeInTheDocument()
    );
    expect(query.mock.calls[0]?.[1]).toEqual(expect.objectContaining({
      query: expect.objectContaining({
        scope: { kind: "form", form_id: "form-Test" },
        filters: [],
        sort: [],
      }),
      projection: expect.objectContaining({
        kind: "fields",
        fields: expect.arrayContaining([
          { kind: "property", field_id: 1 },
        ]),
      }),
    }));

    fireEvent.click(getByRole("button", { name: "Next" }));
    await waitFor(() =>
      expect(desktopTable().getByText("entry-2"))
        .toBeInTheDocument()
    );
    expect(query.mock.calls.at(-1)?.[1].after).toBe("cursor-1");

    fireEvent.click(getByRole("button", { name: "Previous" }));
    await waitFor(() =>
      expect(desktopTable().getByText("entry-1"))
        .toBeInTheDocument()
    );
    expect(query.mock.calls.at(-1)?.[1].after).toBeUndefined();
  });

  it("formats structured fields safely and keeps them read-only", async () => {
    const entryForm = {
      name: "Test",
      fields: {
        asset: { type: "asset_reference" },
        metadata: { type: "object" },
      },
    } as any;
    mockEntryQuery([{
      id: "1",
      properties: {
        asset: {
          asset_id: "asset-1",
          name: "hero-banner.png",
          media_type: "image/png",
          size_bytes: 1_800_000,
          sha256: "a".repeat(64),
        },
        metadata: { source: "import" },
      },
      updated_at: "2026-01-01",
    }] as any);

    const { getByTitle } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
      />
    ));

    await waitFor(() => {
      expect(desktopTable().getByText(/PNG ·/)).toBeInTheDocument();
      expect(desktopTable().getByText("-")).toBeInTheDocument();
    });

    fireEvent.click(getByTitle("Enable Editing"));
    fireEvent.click(desktopTable().getByText(/PNG ·/));
    expect(desktopTable().queryByDisplayValue(/PNG/)).not.toBeInTheDocument();
    expect(document.body.textContent).not.toContain("[object Object]");
  });

  it("REQ-FE-019: sends multi-column sort to EntryQuery", async () => {
    const entryForm = {
      name: "Test",
      fields: {
        price: { type: "double" },
        status: { type: "string" },
      },
    } as any;
    const entries = [
      {
        id: "entry-b",
        properties: { price: 20, status: "open" },
        updated_at: "2026-01-01",
      },
      {
        id: "entry-a",
        properties: { price: 10, status: "closed" },
        updated_at: "2026-01-02",
      },
    ];

    const query = vi.spyOn(entryApi, "query").mockImplementation(
      async (_spaceId, request) => {
        const direction = request.query.sort[0]?.direction;
        const sorted = direction === "asc"
          ? [entries[1], entries[0]]
          : direction === "desc"
          ? [entries[0], entries[1]]
          : entries;
        return entryQueryPage(sorted);
      },
    );
    render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() =>
      expect(desktopTable().getByText("entry-a")).toBeInTheDocument()
    );

    const priceHeader = desktopTable().getByText("price");
    fireEvent.click(priceHeader); // Asc null -> asc

    await waitFor(() => {
      const request = query.mock.calls.at(-1)?.[1];
      expect(request?.query.sort).toEqual([{
        field: { kind: "property", field_id: 1 },
        direction: "asc",
      }]);
      expect(desktopTable().getByText("entry-a")).toBeInTheDocument();
    });

    fireEvent.click(desktopTable().getByText("status"));
    await waitFor(() => {
      expect(query.mock.calls.at(-1)?.[1].query.sort).toEqual([
        {
          field: { kind: "property", field_id: 1 },
          direction: "asc",
        },
        {
          field: { kind: "property", field_id: 2 },
          direction: "asc",
        },
      ]);
    });

    fireEvent.click(priceHeader); // Asc -> desc
    await waitFor(() => {
      const request = query.mock.calls.at(-1)?.[1];
      expect(request?.query.sort).toEqual([{
        field: { kind: "property", field_id: 1 },
        direction: "desc",
      }, {
        field: { kind: "property", field_id: 2 },
        direction: "asc",
      }]);
    });

    fireEvent.click(priceHeader); // desc -> remove, retaining the second key
    await waitFor(() => {
      expect(query.mock.calls.at(-1)?.[1].query.sort).toEqual([{
        field: { kind: "property", field_id: 2 },
        direction: "asc",
      }]);
    });

    fireEvent.click(desktopTable().getByText("status"));
    fireEvent.click(desktopTable().getByText("status"));
    await waitFor(() => {
      expect(query.mock.calls.at(-1)?.[1].query.sort).toEqual([]);
    });
  });

  it("REQ-FE-020: sends global text search to EntryQuery", async () => {
    const entryForm = {
      name: "Test",
      fields: { tag: { type: "string" } },
    } as any;
    const entries = [
      {
        id: "entry-apple",
        properties: { tag: "fruit" },
        updated_at: "2026-01-01",
      },
      {
        id: "entry-carrot",
        properties: { tag: "veggie" },
        updated_at: "2026-01-01",
      },
    ];

    const query = vi.spyOn(entryApi, "query").mockImplementation(
      async (_spaceId, request) => {
        const text = request.query.text ?? "";
        return entryQueryPage(
          entries.filter((entry) =>
            String(entry.properties?.tag ?? "").includes(text)
          ),
        );
      },
    );
    const { getByPlaceholderText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() =>
      expect(desktopTable().getByText("entry-apple")).toBeInTheDocument()
    );

    const searchInput = getByPlaceholderText("Global Search...");
    fireEvent.input(searchInput, { target: { value: "veggie" } });

    await waitFor(() => {
      expect(query.mock.calls.at(-1)?.[1].query.text).toBe("veggie");
      expect(desktopTable().getByText("entry-carrot")).toBeInTheDocument();
      expect(desktopTable().queryByText("entry-apple")).not.toBeInTheDocument();
    });
  });

  it("REQ-FE-020: preserves Rust-owned numeric filter types", async () => {
    const entryForm = {
      name: "Test",
      fields: {
        count: {
          type: "long",
          query_capability: {
            field: { kind: "property", field_id: 1 },
            name: "count",
            field_type: "long",
            filterable: true,
            sortable: true,
            projectable: true,
            supported_operators: ["equals", "lt", "lte", "gt", "gte"],
          },
        },
        ratio: {
          type: "double",
          query_capability: {
            field: { kind: "property", field_id: 2 },
            name: "ratio",
            field_type: "double",
            filterable: true,
            sortable: true,
            projectable: true,
            supported_operators: ["equals", "lt", "lte", "gt", "gte"],
          },
        },
      },
    } as any;
    const query = mockEntryQuery([{
      id: "entry-1",
      properties: { count: 42, ratio: 1.5 },
      updated_at: "2026-01-01",
    }]);

    render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() => expect(document.querySelector("tbody")).toBeTruthy());
    const filterInputs = document.querySelectorAll("input.ui-table-filter");
    fireEvent.input(filterInputs[0], { target: { value: "42" } });
    await waitFor(() => {
      expect(query.mock.calls.at(-1)?.[1].query.filters[0]?.value).toBe(42);
    });

    fireEvent.input(filterInputs[1], { target: { value: "1.5" } });
    await waitFor(() => {
      expect(query.mock.calls.at(-1)?.[1].query.filters).toEqual([
        {
          field: { kind: "property", field_id: 1 },
          operator: "equals",
          value: 42,
        },
        {
          field: { kind: "property", field_id: 2 },
          operator: "equals",
          value: 1.5,
        },
      ]);
    });
  });

  it("REQ-FE-020: combines global and column filters", async () => {
    const entryForm = {
      name: "Test",
      fields: { tag: { type: "string" } },
    } as any;
    const entries = [
      {
        id: "apple-1",
        properties: { tag: "fruit" },
        updated_at: "2026-01-01",
      },
      {
        id: "apple-2",
        properties: { tag: "dessert" },
        updated_at: "2026-01-01",
      },
      {
        id: "carrot-1",
        properties: { tag: "fruit" },
        updated_at: "2026-01-01",
      },
    ];

    const query = vi.spyOn(entryApi, "query").mockImplementation(
      async (_spaceId, request) => {
        const text = request.query.text ?? "";
        const filtered = entries.filter((entry) =>
          (!text || entry.id.includes(text)) &&
          request.query.filters.every((filter) =>
            String(entry.properties?.tag ?? "").includes(String(filter.value))
          )
        );
        return entryQueryPage(filtered);
      },
    );
    const { getByPlaceholderText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() => {
      expect(document.querySelectorAll("tbody tr").length).toBe(3);
    });

    fireEvent.input(getByPlaceholderText("Global Search..."), {
      target: { value: "apple" },
    });
    const columnFilters = document.querySelectorAll("input.ui-table-filter");
    fireEvent.input(columnFilters[0], { target: { value: "fruit" } });

    await waitFor(() => {
      expect(query.mock.calls.at(-1)?.[1].query).toEqual({
        scope: { kind: "form", form_id: "form-Test" },
        text: "apple",
        filters: [{
          field: { kind: "property", field_id: 1 },
          operator: "contains",
          value: "fruit",
        }],
        sort: [],
      });
      const rows = document.querySelectorAll("tbody tr");
      expect(rows.length).toBe(1);
      expect(rows[0]).toHaveTextContent("apple-1");
      expect(rows[0]).toHaveTextContent("fruit");
    });
  });

  it("REQ-FE-021: exports filtered data to CSV", async () => {
    const entryForm = {
      name: "Test",
      fields: { price: { type: "double" } },
    } as any;
    const entries = [
      {
        id: "keep-entry",
        properties: { price: 100 },
        updated_at: "2026-01-01",
      },
      {
        id: "drop-entry",
        properties: { price: 200 },
        updated_at: "2026-01-01",
      },
    ];

    vi.spyOn(entryApi, "query").mockImplementation(
      async (_spaceId, request) =>
        entryQueryPage(
          request.query.text
            ? entries.filter((entry) => entry.id.includes(request.query.text!))
            : entries,
        ),
    );

    // Mock URL.createObjectURL/revokeObjectURL
    let exportedBlob: Blob | undefined;
    const createSpy = vi.fn().mockImplementation((blob: Blob) => {
      exportedBlob = blob;
      return "blob:test";
    });
    const revokeSpy = vi.fn();
    global.URL.createObjectURL = createSpy;
    global.URL.revokeObjectURL = revokeSpy;

    // Mock anchor element
    const linkClickSpy = vi.fn();
    const originalCreateElement = document.createElement;
    vi.spyOn(document, "createElement").mockImplementation((tag) => {
      const el = originalCreateElement.call(document, tag);
      if (tag === "a") {
        (el as any).click = linkClickSpy;
      }
      return el;
    });

    const { getByPlaceholderText, getByText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() => expect(getByText("Export CSV")).toBeInTheDocument());

    fireEvent.input(getByPlaceholderText("Global Search..."), {
      target: { value: "keep" },
    });
    await waitFor(() => {
      const rows = document.querySelectorAll("tbody tr");
      expect(rows.length).toBe(1);
      expect(rows[0]).toHaveTextContent("keep-entry");
    });

    fireEvent.click(getByText("Export CSV"));

    await waitFor(() => {
      expect(createSpy).toHaveBeenCalled();
      expect(linkClickSpy).toHaveBeenCalled();
      expect(exportedBlob).toBeInstanceOf(Blob);
    });
    const csvContent = await exportedBlob!.text();
    expect(csvContent).toContain(
      '"keep-entry","100","2026-01-01T00:00:00.000Z"',
    );
    expect(csvContent).not.toContain("drop-entry");
  });

  it("exports formula/control-prefixed values as literal text with CRLF endings", async () => {
    const entryForm = {
      name: "Test",
      fields: { code: { type: "string" } },
    } as any;
    const entries = [
      {
        id: "=SUM(A1:A2)",
        properties: { code: "+1" },
        updated_at: "2026-01-01",
      },
      {
        id: "-discount",
        properties: { code: "@user" },
        updated_at: "2026-01-02",
      },
      {
        id: "plain",
        properties: { code: "\u0001=CMD" },
        updated_at: "2026-01-03",
      },
    ];
    const snapshot = JSON.parse(JSON.stringify(entries));

    mockEntryQuery(entries);

    let exportedBlob: Blob | undefined;
    global.URL.createObjectURL = vi.fn().mockImplementation((blob: Blob) => {
      exportedBlob = blob;
      return "blob:test";
    });
    global.URL.revokeObjectURL = vi.fn();
    const linkClickSpy = vi.fn();
    // Bind the prototype original: an earlier test in this file already
    // spies on document.createElement, so capturing document.createElement
    // here would recurse into that mock.
    const originalCreateElement = Document.prototype.createElement.bind(
      document,
    );
    vi.spyOn(document, "createElement").mockImplementation(
      ((tag: string) => {
        const el = originalCreateElement(tag);
        if (tag === "a") {
          (el as any).click = linkClickSpy;
        }
        return el;
      }) as typeof document.createElement,
    );

    render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() =>
      expect(desktopTable().getByText("plain")).toBeInTheDocument()
    );
    fireEvent.click(screen.getByText("Export CSV"));

    await waitFor(() => {
      expect(linkClickSpy).toHaveBeenCalled();
      expect(exportedBlob).toBeInstanceOf(Blob);
    });
    const csvContent = await exportedBlob!.text();
    expect(csvContent).toBe(
      '"id","code","updated_at"\r\n' +
        '"\'=SUM(A1:A2)","\'+\u0031","2026-01-01T00:00:00.000Z"\r\n' +
        '"\'-discount","\'@user","2026-01-02T00:00:00.000Z"\r\n' +
        '"plain","\'\u0001=CMD","2026-01-03T00:00:00.000Z"',
    );
    // The export is a derived representation: durable Entry values are unchanged.
    expect(entries).toEqual(snapshot);
    expect(desktopTable().getByText("=SUM(A1:A2)")).toBeInTheDocument();
    expect(desktopTable().getByText("-discount")).toBeInTheDocument();
    vi.mocked(document.createElement).mockRestore();
  });

  it("splits CSV exports into bounded WASM requests without changing bytes", async () => {
    const headers = ["id"];
    const rows = [["a"], ["b"], ["c"]];
    const singleRowBytes = spreadsheetCsvRequestBytes([headers, rows[0]]);

    const chunks = chunkCsvRowsForExport(headers, rows, singleRowBytes);
    expect(chunks.length).toBeGreaterThan(1);
    expect(chunks.flat()).toEqual(rows);
    for (const [index, chunk] of chunks.entries()) {
      const invocation = index === 0 ? [headers, ...chunk] : chunk;
      expect(spreadsheetCsvRequestBytes(invocation)).toBeLessThanOrEqual(
        singleRowBytes,
      );
    }

    await expect(
      encodeSpreadsheetCsvChunked(headers, rows, singleRowBytes),
    ).resolves.toBe(await encodeSpreadsheetCsv([headers, ...rows]));
  });

  it("bounds data-only CSV pages without repeating the header", async () => {
    const rows = [["a"], ["b"], ["c"]];
    const limit = spreadsheetCsvRequestBytes([["a"]]);
    const chunks = chunkCsvDataRowsForExport(rows, limit);

    expect(chunks.flat()).toEqual(rows);
    expect(chunks.length).toBeGreaterThan(1);
    for (const chunk of chunks) {
      expect(spreadsheetCsvRequestBytes(chunk)).toBeLessThanOrEqual(limit);
    }
    await expect(encodeSpreadsheetCsvDataRowsChunked(rows, limit)).resolves
      .toBe(
        await encodeSpreadsheetCsv(rows),
      );
  });

  it("measures the exact envelope near the ASCII boundary", () => {
    const headers = ["id"];
    const fitRows = [[headers, ["a"]].flat()];
    const fitBytes = spreadsheetCsvRequestBytes([headers, ["a"]]);
    // Fits at the exact limit: one chunk.
    expect(chunkCsvRowsForExport(headers, [["a"]], fitBytes)).toEqual([
      [["a"]],
    ]);
    // One more byte forces the next chunk under exact accounting.
    const oneMore = [["a"], ["b"]];
    const chunks = chunkCsvRowsForExport(headers, oneMore, fitBytes);
    expect(chunks.length).toBe(2);
    expect(chunks.flat()).toEqual(oneMore);
    for (const [index, chunk] of chunks.entries()) {
      const invocation = index === 0 ? [headers, ...chunk] : chunk;
      expect(spreadsheetCsvRequestBytes(invocation)).toBeLessThanOrEqual(
        fitBytes,
      );
    }
    void fitRows;
  });

  it("measures the exact envelope near the multibyte JA boundary", () => {
    const headers = ["id"];
    const row: readonly string[] = ["日本語"];
    const fitBytes = spreadsheetCsvRequestBytes([headers, row]);
    expect(chunkCsvRowsForExport(headers, [row], fitBytes)).toEqual([[row]]);

    const twoRows: readonly (readonly string[])[] = [row, ["日本語追"]];
    const chunks = chunkCsvRowsForExport(headers, twoRows, fitBytes);
    expect(chunks.flat()).toEqual(twoRows);
    expect(chunks.length).toBe(2);
    for (const [index, chunk] of chunks.entries()) {
      const invocation = index === 0 ? [headers, ...chunk] : chunk;
      expect(spreadsheetCsvRequestBytes(invocation)).toBeLessThanOrEqual(
        fitBytes,
      );
    }
  });

  it("keeps every invocation within 256KiB under exact accounting", async () => {
    const headers = ["id", "body"];
    const rows = Array.from(
      { length: 20 },
      (_, i) => [`row-${i}`, `value-${i}`] as const,
    ).map(([a, b]) => [a, b] as unknown as readonly string[]);
    const chunks = chunkCsvRowsForExport(headers, rows);
    expect(chunks.flat()).toEqual(rows);
    for (const [index, chunk] of chunks.entries()) {
      const invocation = index === 0 ? [headers, ...chunk] : chunk;
      expect(spreadsheetCsvRequestBytes(invocation)).toBeLessThanOrEqual(
        256 * 1024,
      );
    }
    await expect(encodeSpreadsheetCsvChunked(headers, rows)).resolves.toBe(
      await encodeSpreadsheetCsv([headers, ...rows]),
    );
  });

  it("REQ-FE-030: Add Row button opens the canonical entry editor", async () => {
    const entryForm = {
      name: "Test",
      fields: { col: { type: "string" } },
    } as any;
    const entries: QueryTestEntry[] = [];
    mockEntryQuery(entries);
    const onAddRow = vi.fn();
    const createSpy = vi.spyOn(entryApi, "create");

    const { getByTitle, getByText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={onAddRow}
      />
    ));

    // Enable editing first
    const toggleButton = getByTitle("Enable Editing");
    fireEvent.click(toggleButton);

    const addButton = getByText("Add Row");
    fireEvent.click(addButton);

    expect(onAddRow).toHaveBeenCalledTimes(1);
    expect(createSpy).not.toHaveBeenCalled();
    createSpy.mockRestore();
  });

  it("REQ-FE-031: Edit Mode toggle and inline edit", async () => {
    const entryForm = {
      name: "Test",
      fields: { col: { type: "string" } },
    } as any;
    const entries = [
      {
        id: "1",
        properties: { col: "val" },
        updated_at: "2026-01-01",
      },
    ];
    mockEntryQuery(entries);
    const getSpy = vi.spyOn(entryApi, "get").mockResolvedValue({
      id: "1",
      form: "Test",
      fields: { col: "val" },
      revision_id: "rev1",
    } as any);
    const updateSpy = vi.spyOn(entryApi, "update").mockResolvedValue({} as any);

    const { getByTitle } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    // Wait for render
    await waitFor(() => desktopTable().getByText("1"));

    // Click Edit Toggle (Lock icon)
    const toggleButton = getByTitle("Enable Editing");
    fireEvent.click(toggleButton);

    // Now find the cell value and it should be an input or become input on click
    const cell = desktopTable().getByText("val");
    fireEvent.click(cell);

    const input = desktopTable().getByDisplayValue("val");
    fireEvent.input(input, { target: { value: "new-val" } });
    fireEvent.blur(input);

    await waitFor(() => {
      expect(updateSpy).toHaveBeenCalledWith(
        "ws",
        "1",
        expect.objectContaining({
          form: "Test",
          fields: { col: "new-val" },
          parent_revision_id: "rev1",
        }),
      );
    });
    updateSpy.mockRestore();
    getSpy.mockRestore();
  });

  it("should have a link icon for navigation and not navigate on row click", async () => {
    const entryForm = {
      name: "Test",
      fields: { col: { type: "string" } },
    } as any;
    const entries = [
      {
        id: "1",
        properties: { col: "val" },
        updated_at: "2026-01-01",
      },
    ];
    mockEntryQuery(entries);
    const onEntryClick = vi.fn();

    render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={onEntryClick}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() =>
      expect(desktopTable().getByText("1")).toBeInTheDocument()
    );

    // Find the row
    const row = desktopTable().getByText("1").closest("tr");
    if (!row) throw new Error("Row not found");

    // Click the row itself (but not the link icon)
    fireEvent.click(row);
    expect(onEntryClick).not.toHaveBeenCalled();

    // Find the link icon (title="View Entry") and click it
    const linkButton = desktopTable().getByRole("button", {
      name: /view entry/i,
    });
    fireEvent.click(linkButton);
    expect(onEntryClick).toHaveBeenCalledWith("1");
  });

  it("should show restricted lock icon when not in edit mode and open lock icon when in edit mode", async () => {
    const entryForm = { name: "Test", fields: {} } as any;
    mockEntryQuery([]);

    const { getByTitle, queryByTitle } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    // Initially Locked
    expect(getByTitle("Locked")).toBeInTheDocument();
    expect(queryByTitle("Unlocked")).not.toBeInTheDocument();

    // Toggle to Editable
    const toggleButton = getByTitle("Enable Editing");
    fireEvent.click(toggleButton);

    expect(getByTitle("Unlocked")).toBeInTheDocument();
    expect(queryByTitle("Locked")).not.toBeInTheDocument();
  });
  it("REQ-FE-031: keyboard copy shortcut", async () => {
    const entryForm = {
      name: "Test",
      fields: { col: { type: "string" } },
    } as any;
    const entries = [
      {
        id: "1",
        properties: { col: "val1" },
        updated_at: new Date("2026-01-01").toISOString(),
      },
      {
        id: "2",
        properties: { col: "val2" },
        updated_at: new Date("2026-01-02").toISOString(),
      },
    ];
    mockEntryQuery(entries);
    const writeTextSpy = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText: writeTextSpy } });

    const { getByText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() => desktopTable().getByText("1"));

    // Simulate drag selection from (0,0) to (1,1)
    // Col 0: ID, Col 1: col
    const cell1 = desktopTable().getByText("1");
    const cell2 = desktopTable().getByText("val2");

    fireEvent.mouseDown(cell1);
    fireEvent.mouseEnter(cell2, { buttons: 1 });
    fireEvent.mouseUp(document);

    // Trigger Ctrl+C
    fireEvent.keyDown(document, { key: "c", ctrlKey: true });

    expect(writeTextSpy).toHaveBeenCalledWith("1\tval1\n2\tval2");
  });

  it("should not trigger custom copy when input is focused", async () => {
    const entryForm = { name: "Test", fields: {} } as any;
    mockEntryQuery([]);
    const writeTextSpy = vi.fn();
    Object.assign(navigator, { clipboard: { writeText: writeTextSpy } });

    const { getByPlaceholderText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    const searchInput = getByPlaceholderText("Global Search...");
    searchInput.focus();

    // We need to trigger the keydown event on document since that's where the listener is
    fireEvent.keyDown(document, { key: "c", ctrlKey: true });

    expect(writeTextSpy).not.toHaveBeenCalled();
  });

  it("sort menu: handleSortFieldChange via dropdown", async () => {
    const entryForm = {
      name: "Test",
      fields: { col: { type: "string" } },
    } as any;
    const entries = [
      {
        id: "entry-b",
        properties: { col: "v1" },
        updated_at: "2026-01-02",
      },
      {
        id: "entry-a",
        properties: { col: "v2" },
        updated_at: "2026-01-01",
      },
    ];
    const query = vi.spyOn(entryApi, "query").mockImplementation(
      async (_spaceId, request) => {
        const direction = request.query.sort[0]?.direction;
        const sorted = direction === "asc"
          ? [entries[1], entries[0]]
          : direction === "desc"
          ? [entries[0], entries[1]]
          : entries;
        return entryQueryPage(sorted);
      },
    );

    const { getByLabelText, getByText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() => desktopTable().getByText("entry-a"));

    // Open sort menu
    fireEvent.click(getByLabelText("Sort menu"));
    // Change sort field via dropdown
    const sortFieldSelect = getByLabelText("Sort field");
    fireEvent.change(sortFieldSelect, { target: { value: "col" } });

    await waitFor(() => {
      expect(query.mock.calls.at(-1)?.[1].query.sort).toEqual([{
        field: { kind: "property", field_id: 1 },
        direction: "asc",
      }]);
    });

    // Change to empty (clears sort)
    fireEvent.change(sortFieldSelect, { target: { value: "" } });
  });

  it("column filter: updateColumnFilter via input", async () => {
    const entryForm = {
      name: "Test",
      fields: { col: { type: "string" } },
    } as any;
    const entries = [
      {
        id: "apple-1",
        properties: { col: "fruit" },
        updated_at: "2026-01-01",
      },
      {
        id: "carrot-1",
        properties: { col: "veggie" },
        updated_at: "2026-01-01",
      },
    ];
    const query = vi.spyOn(entryApi, "query").mockImplementation(
      async (_spaceId, request) => {
        const filter = request.query.filters[0]?.value;
        return entryQueryPage(
          filter
            ? entries.filter((entry) =>
              String(entry.properties?.col).includes(String(filter))
            )
            : entries,
        );
      },
    );

    render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() => expect(document.querySelector("tbody")).toBeTruthy());

    // Column filters are visible (showColumnFilters starts true)
    // Find the canonical property filter.
    const filterInputs = document.querySelectorAll("input.ui-table-filter");
    expect(filterInputs.length).toBeGreaterThan(0);

    fireEvent.input(filterInputs[0], { target: { value: "fruit" } });

    await waitFor(() => {
      expect(query.mock.calls.at(-1)?.[1].query.filters).toEqual([{
        field: { kind: "property", field_id: 1 },
        operator: "contains",
        value: "fruit",
      }]);
      const rows = document.querySelectorAll("tbody tr");
      expect(rows.length).toBe(1);
    });
  });

  it("copy includes updated_at column when selected", async () => {
    const entryForm = {
      name: "Test",
      fields: { col: { type: "string" } },
    } as any;
    const entries = [
      {
        id: "1",
        properties: { col: "val1" },
        updated_at: new Date("2026-01-01").toISOString(),
      },
    ];
    mockEntryQuery(entries);
    const writeTextSpy = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText: writeTextSpy } });

    const { getByText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() => desktopTable().getByText("1"));

    // Select ID cell (col 0)
    const cell1 = desktopTable().getByText("1");
    // Get a cell with a date (updated_at column, col 2)
    const updatedCell = document.querySelectorAll("tbody td")[3]; // Actions(0), ID(1), col(2), updated(3)

    fireEvent.mouseDown(cell1);
    fireEvent.mouseEnter(updatedCell, { buttons: 1 });
    fireEvent.mouseUp(document);

    fireEvent.keyDown(document, { key: "c", ctrlKey: true });

    await waitFor(() => {
      expect(writeTextSpy).toHaveBeenCalled();
    });
  });

  it("id cell is read-only via handleCellUpdate", async () => {
    const entryForm = {
      name: "Test",
      fields: { col: { type: "string" } },
    } as any;
    const entries = [
      {
        id: "entry-1",
        properties: { col: "val" },
        updated_at: "2026-01-01",
      },
    ];
    mockEntryQuery(entries);
    const getSpy = vi.spyOn(entryApi, "get").mockResolvedValue({
      id: "entry-1",
      fields: { col: "val" },
      revision_id: "rev1",
    } as any);
    const updateSpy = vi.spyOn(entryApi, "update").mockResolvedValue({} as any);

    const { getByText, getByTitle } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() => desktopTable().getByText("entry-1"));

    // Enable edit mode
    fireEvent.click(getByTitle("Enable Editing"));

    // Click on the ID cell td: the stable entry id is identity and never
    // becomes an inline editor.
    const idText = desktopTable().getByText("entry-1");
    const idTd = idText.closest("td") ?? idText;
    fireEvent.click(idTd);

    // No inline editor opens for the ID cell.
    expect(
      document.querySelector("input.ui-table-cell-input"),
    ).not.toBeInTheDocument();
    expect(updateSpy).not.toHaveBeenCalled();

    updateSpy.mockRestore();
    getSpy.mockRestore();
  });

  it("renders a mobile card with primary fields and progressively discloses the rest", async () => {
    const entryForm = {
      name: "Test",
      fields: {
        status: { type: "string" },
        owner: { type: "string" },
        priority: { type: "string" },
        notes: { type: "markdown" },
      },
    } as any;
    mockEntryQuery([{
      id: "entry-1",
      properties: {
        status: "Open",
        owner: "Aki",
        priority: "High",
        notes: "Longer context",
      },
      updated_at: "2026-01-01",
    }] as any);
    const onEntryClick = vi.fn();

    render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={onEntryClick}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() =>
      expect(mobileList().getByText("entry-1"))
        .toBeInTheDocument()
    );
    expect(mobileList().getByText("status")).toBeInTheDocument();
    expect(mobileList().getByText("priority")).toBeInTheDocument();
    const extraField = mobileList().getByText("notes");
    expect(extraField.closest(".ui-table-mobile-extra-fields"))
      .toBeTruthy();
    expect(mobileList().getByText("Show 1 more field")).toBeInTheDocument();
    // The mobile card header shows the stable entry ID and navigates.
    fireEvent.click(mobileList().getByText("entry-1"));
    expect(onEntryClick).toHaveBeenCalledWith("entry-1");
  });

  it("keeps additional mobile card fields inline-editable", async () => {
    const entryForm = {
      name: "Test",
      fields: {
        status: { type: "string" },
        owner: { type: "string" },
        priority: { type: "string" },
        notes: { type: "string" },
      },
    } as any;
    mockEntryQuery([{
      id: "1",
      properties: {
        status: "Open",
        owner: "Aki",
        priority: "High",
        notes: "Old",
      },
      updated_at: "2026-01-01",
    }] as any);
    vi.spyOn(entryApi, "get").mockResolvedValue({
      id: "1",
      form: "Test",
      fields: { notes: "Old" },
      revision_id: "rev1",
    } as any);
    const updateSpy = vi.spyOn(entryApi, "update").mockResolvedValue({} as any);

    const { getByTitle } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() =>
      expect(mobileList().getByText("1"))
        .toBeInTheDocument()
    );
    fireEvent.click(getByTitle("Enable Editing"));
    fireEvent.click(mobileList().getByRole("button", { name: "Old" }));
    const notesInput = mobileList().getByDisplayValue("Old");
    fireEvent.input(notesInput, { target: { value: "New" } });
    fireEvent.blur(notesInput);

    await waitFor(() => {
      expect(updateSpy).toHaveBeenCalledWith(
        "ws",
        "1",
        expect.objectContaining({
          form: "Test",
          fields: { notes: "New" },
          parent_revision_id: "rev1",
        }),
      );
    });
  });

  it("exposes the filter toggle with expanded state and controlled regions", async () => {
    const entryForm = {
      name: "Test",
      fields: { col: { type: "string" } },
    } as any;
    mockEntryQuery([]);

    const { getByRole, getAllByRole } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() => expect(document.querySelector("tbody")).toBeTruthy());

    const toggle = getByRole("button", { name: "Filter" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const controls = (toggle.getAttribute("aria-controls") ?? "").split(/\s+/);
    expect(controls).toContain("form-table-mobile-filters");
    expect(controls).toContain("form-table-desktop");
    for (const id of controls) {
      expect(document.getElementById(id)).not.toBeNull();
    }

    expect(getByRole("textbox", { name: "Global Search..." }))
      .toBeInTheDocument();
    // Desktop header inputs and the mobile panel share accessible names.
    expect(getAllByRole("textbox", { name: "col Filter..." }).length)
      .toBeGreaterThan(0);
  });

  it("restores focus to the filter toggle when panels close under focus", async () => {
    const entryForm = {
      name: "Test",
      fields: { col: { type: "string" } },
    } as any;
    mockEntryQuery([]);

    const { getByRole } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={canonicalForm(entryForm)}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() => expect(document.querySelector("tbody")).toBeTruthy());

    const toggle = getByRole("button", { name: "Filter" });
    const filterInput = document.querySelector(
      "#form-table-mobile-filters input",
    ) as HTMLInputElement;
    expect(filterInput).not.toBeNull();
    filterInput.focus();
    expect(filterInput).toHaveFocus();

    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toHaveFocus());
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(
      document.querySelector("#form-table-mobile-filters input"),
    ).toBeNull();
  });
});
