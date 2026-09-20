import "@testing-library/jest-dom/vitest";
import { describe, expect, it, vi } from "vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import {
  chunkCsvRowsForExport,
  encodeSpreadsheetCsvChunked,
  FormTable,
} from "./FormTable";
import {
  encodeSpreadsheetCsv,
  entryApi,
  spreadsheetCsvRequestBytes,
} from "~/lib/ugoite-client";
import { searchApi } from "~/lib/ugoite-client";
import { setLocale } from "~/lib/i18n";

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
  });

  it("keeps the Form workspace available when its query fails", async () => {
    const entryForm = { name: "Entry", fields: {} } as any;
    const query = vi.spyOn(searchApi, "query")
      .mockRejectedValueOnce(new Error("Internal server error"))
      .mockResolvedValue([] as any);

    const { getByRole, getByText, queryByRole } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
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

    const spy = vi.spyOn(searchApi, "query").mockResolvedValue(entries as any);

    render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
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

  it("formats structured fields safely and keeps them read-only", async () => {
    const entryForm = {
      name: "Test",
      fields: {
        asset: { type: "asset_reference" },
        metadata: { type: "object" },
      },
    } as any;
    vi.spyOn(searchApi, "query").mockResolvedValue([{
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
        entryForm={entryForm}
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

  it("REQ-FE-019: sorts entries when clicking headers", async () => {
    const entryForm = {
      name: "Test",
      fields: { price: { type: "number" } },
    } as any;
    const entries = [
      {
        id: "entry-b",
        properties: { price: 20 },
        updated_at: "2026-01-01",
      },
      {
        id: "entry-a",
        properties: { price: 10 },
        updated_at: "2026-01-02",
      },
    ];

    vi.spyOn(searchApi, "query").mockResolvedValue(entries as any);
    const { getByText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() =>
      expect(desktopTable().getByText("entry-a")).toBeInTheDocument()
    );

    // Initially might be in order returned by API. Click ID to sort.
    const idHeader = desktopTable().getByText("ID");
    fireEvent.click(idHeader); // Asc null -> asc

    await waitFor(() => {
      const rows = document.querySelectorAll("tbody tr");
      expect(rows[0]).toHaveTextContent("entry-a");
    });

    fireEvent.click(idHeader); // Asc -> desc
    await waitFor(() => {
      const rows = document.querySelectorAll("tbody tr");
      expect(rows[0]).toHaveTextContent("entry-b");
    });

    fireEvent.click(idHeader); // desc -> null (clear sort)
    await waitFor(() => {
      // Both entries still visible (sort cleared)
      const rows = document.querySelectorAll("tbody tr");
      expect(rows.length).toBe(2);
    });
  });

  it("REQ-FE-020: filters entries globally", async () => {
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

    vi.spyOn(searchApi, "query").mockResolvedValue(entries as any);
    const { getByPlaceholderText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() =>
      expect(desktopTable().getByText("entry-apple")).toBeInTheDocument()
    );

    const searchInput = getByPlaceholderText("Global Search...");
    fireEvent.input(searchInput, { target: { value: "carrot" } });

    await waitFor(() => {
      expect(desktopTable().getByText("entry-carrot")).toBeInTheDocument();
      expect(desktopTable().queryByText("entry-apple")).not.toBeInTheDocument();
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

    vi.spyOn(searchApi, "query").mockResolvedValue(entries as any);
    const { getByPlaceholderText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
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
    fireEvent.input(columnFilters[1], { target: { value: "fruit" } });

    await waitFor(() => {
      const rows = document.querySelectorAll("tbody tr");
      expect(rows.length).toBe(1);
      expect(rows[0]).toHaveTextContent("apple-1");
      expect(rows[0]).toHaveTextContent("fruit");
    });
  });

  it("REQ-FE-021: exports filtered data to CSV", async () => {
    const entryForm = {
      name: "Test",
      fields: { price: { type: "number" } },
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

    vi.spyOn(searchApi, "query").mockResolvedValue(entries as any);

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
        entryForm={entryForm}
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

    vi.spyOn(searchApi, "query").mockResolvedValue(entries as any);

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
        entryForm={entryForm}
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
    const entries = [];
    vi.spyOn(searchApi, "query").mockResolvedValue(entries as any);
    const onAddRow = vi.fn();
    const createSpy = vi.spyOn(entryApi, "create");

    const { getByTitle, getByText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
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
    vi.spyOn(searchApi, "query").mockResolvedValue(entries as any);
    const getSpy = vi.spyOn(entryApi, "get").mockResolvedValue({
      id: "1",
      form: "Test",
      sections: { col: "val" },
      revision_id: "rev1",
    } as any);
    const updateSpy = vi.spyOn(entryApi, "update").mockResolvedValue({} as any);

    const { getByTitle } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
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
    vi.spyOn(searchApi, "query").mockResolvedValue(entries as any);
    const onEntryClick = vi.fn();

    render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
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
    vi.spyOn(searchApi, "query").mockResolvedValue([] as any);

    const { getByTitle, queryByTitle } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
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
    vi.spyOn(searchApi, "query").mockResolvedValue(entries as any);
    const writeTextSpy = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText: writeTextSpy } });

    const { getByText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
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
    vi.spyOn(searchApi, "query").mockResolvedValue([] as any);
    const writeTextSpy = vi.fn();
    Object.assign(navigator, { clipboard: { writeText: writeTextSpy } });

    const { getByPlaceholderText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
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
    vi.spyOn(searchApi, "query").mockResolvedValue(entries as any);

    const { getByLabelText, getByText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() => desktopTable().getByText("entry-a"));

    // Open sort menu
    fireEvent.click(getByLabelText("Sort menu"));
    // Change sort field via dropdown
    const sortFieldSelect = getByLabelText("Sort field");
    fireEvent.change(sortFieldSelect, { target: { value: "id" } });

    await waitFor(() => {
      const rows = document.querySelectorAll("tbody tr");
      expect(rows[0]).toHaveTextContent("entry-a");
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
    vi.spyOn(searchApi, "query").mockResolvedValue(entries as any);

    render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
        onEntryClick={() => {}}
        onAddRow={() => {}}
      />
    ));

    await waitFor(() => expect(document.querySelector("tbody")).toBeTruthy());

    // Column filters are visible (showColumnFilters starts true)
    // Find the ID column filter (first filter input after headers)
    const filterInputs = document.querySelectorAll("input.ui-table-filter");
    expect(filterInputs.length).toBeGreaterThan(0);

    // Filter by ID column
    fireEvent.input(filterInputs[0], { target: { value: "apple-1" } });

    await waitFor(() => {
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
    vi.spyOn(searchApi, "query").mockResolvedValue(entries as any);
    const writeTextSpy = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText: writeTextSpy } });

    const { getByText } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
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
    vi.spyOn(searchApi, "query").mockResolvedValue(entries as any);
    const getSpy = vi.spyOn(entryApi, "get").mockResolvedValue({
      id: "entry-1",
      content: "---\nform: Test\n---\n\n## col\nval",
      revision_id: "rev1",
    } as any);
    const updateSpy = vi.spyOn(entryApi, "update").mockResolvedValue({} as any);

    const { getByText, getByTitle } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
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
    vi.spyOn(searchApi, "query").mockResolvedValue([{
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
        entryForm={entryForm}
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
    vi.spyOn(searchApi, "query").mockResolvedValue([{
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
      sections: { notes: "Old" },
      revision_id: "rev1",
    } as any);
    const updateSpy = vi.spyOn(entryApi, "update").mockResolvedValue({} as any);

    const { getByTitle } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
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
    vi.spyOn(searchApi, "query").mockResolvedValue([] as any);

    const { getByRole, getAllByRole } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
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

    expect(getByRole("textbox", { name: "Global Search..." })).toBeInTheDocument();
    // Desktop header inputs and the mobile panel share accessible names.
    expect(getAllByRole("textbox", { name: "ID Filter..." }).length).toBeGreaterThan(0);
    expect(getAllByRole("textbox", { name: "col Filter..." }).length).toBeGreaterThan(0);
  });

  it("restores focus to the filter toggle when panels close under focus", async () => {
    const entryForm = {
      name: "Test",
      fields: { col: { type: "string" } },
    } as any;
    vi.spyOn(searchApi, "query").mockResolvedValue([] as any);

    const { getByRole } = render(() => (
      <FormTable
        spaceId="ws"
        entryForm={entryForm}
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
