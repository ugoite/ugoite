import {
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  untrack,
} from "solid-js";
import type { EntryRecord, Form } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import {
  encodeSpreadsheetCsv,
  entryApi,
  searchApi,
  spreadsheetCsvRequestBytes,
} from "~/lib/ugoite-client";
import { updateH2Section } from "~/lib/markdown";
import { t } from "~/lib/i18n";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { formatDateLabel } from "~/lib/date-format";
import {
  formatValueForDisplay,
  formatValueForInput,
  isPlainEditableValue,
} from "~/lib/display-value";

interface FormTableProps {
  spaceId: string;
  entryForm: Form;
  onEntryClick: (entryId: string) => void;
  onAddRow?: () => void;
}

type SortDirection = "asc" | "desc" | null;

/** Helper to filter entries by ID or properties */
function filterEntries(
  entries: EntryRecord[],
  fields: string[],
  query: string,
) {
  if (!query) return entries;
  const text = query.toLowerCase();
  return entries.filter((entry) => {
    /* v8 ignore start */
    if (entry.id.toLowerCase().includes(text)) return true;
    for (const field of fields) {
      const val = formatValueForDisplay(entry.properties?.[field], "en-US", "")
        .toLowerCase();
      if (val.includes(text)) return true;
    }
    /* v8 ignore stop */
    return false;
  });
}

/** Helper for column-specific filtering */
function applyColumnFilters(
  entries: EntryRecord[],
  filters: Record<string, string>,
) {
  /* v8 ignore next */
  const activeFilters = Object.entries(filters).filter(([_, val]) => !!val);
  if (activeFilters.length === 0) return entries;

  return entries.filter((entry) => {
    for (const [field, filter] of activeFilters) {
      const filterLower = filter.toLowerCase();
      let val = "";
      /* v8 ignore start */
      if (field === "id") val = entry.id;
      else if (field === "updated_at") {
        val = formatDateLabel(entry.updated_at);
      } else {
        val = formatValueForDisplay(entry.properties?.[field], "en-US", "");
      }
      /* v8 ignore stop */

      if (!val.toLowerCase().includes(filterLower)) return false;
    }
    return true;
  });
}

function sortEntries(
  entries: EntryRecord[],
  field: string,
  direction: SortDirection,
) {
  if (!field || !direction) return entries;
  return [...entries].sort((a, b) => {
    let valA: string | number;
    let valB: string | number;

    if (field === "id") {
      /* v8 ignore start */
      valA = a.id;
      valB = b.id;
      /* v8 ignore stop */
      /* v8 ignore start */
    } else if (field === "updated_at") {
      valA = a.updated_at;
      valB = b.updated_at;
    } else {
      const propertyA = a.properties?.[field];
      const propertyB = b.properties?.[field];
      valA = typeof propertyA === "number"
        ? propertyA
        : formatValueForDisplay(propertyA, "en-US", "");
      valB = typeof propertyB === "number"
        ? propertyB
        : formatValueForDisplay(propertyB, "en-US", "");
    }
    /* v8 ignore stop */

    /* v8 ignore start */
    if (valA < valB) return direction === "asc" ? -1 : 1;
    if (valA > valB) return direction === "asc" ? 1 : -1;
    /* v8 ignore stop */
    /* v8 ignore start */
    return 0;
    /* v8 ignore stop */
  });
}

function SortIcon(props: { active: boolean; direction: SortDirection }) {
  /* v8 ignore start */
  if (!props.active || !props.direction) {
    return (
      <svg
        class="w-4 h-4 ui-muted opacity-50"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <title>{t("formTable.sort")}</title>
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          stroke-width="2"
          d="M7 16V4m0 0L3 8m4-4l4 4m6 0v12m0 0l4-4m-4 4l-4-4"
        />
      </svg>
    );
  }
  if (props.direction === "asc") {
    return (
      <svg
        class="w-4 h-4 ui-focus-text"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <title>{t("formTable.ascending")}</title>
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          stroke-width="2"
          d="M3 4h13M3 8h9m-9 4h6m4 0l4-4m0 0l4 4m-4-4v12"
        />
      </svg>
    );
  }
  return (
    <svg
      class="w-4 h-4 ui-focus-text"
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <title>{t("formTable.descending")}</title>
      <path
        stroke-linecap="round"
        stroke-linejoin="round"
        stroke-width="2"
        d="M3 4h13M3 8h9m-9 4h5m1 5v6m0 0l4-4m-4 4l-4-4"
      />
    </svg>
  );
}
/* v8 ignore stop */

/** Build the display values for one entry; shared Rust owns CSV encoding. */
/* v8 ignore start */
function formatCsvValues(entry: EntryRecord, headers: string[]) {
  return headers
    .map((field) => {
      let val = "";
      if (field === "id") val = entry.id;
      else if (field === "updated_at") {
        try {
          val = new Date(entry.updated_at).toISOString();
        } catch {
          val = entry.updated_at;
        }
      } else {
        val = formatValueForDisplay(entry.properties?.[field], "en-US", "");
      }
      return val;
    });
}
/* v8 ignore stop */

/**
 * Per-call budget for one `encodeSpreadsheetCsv` WASM request. Mirrors the
 * 256 KiB JSON protocol input limit enforced by the Rust/WASM bridge, so a
 * large Form export stays a bounded sequence of small requests instead of a
 * single oversized one. CSV encoding is row-independent (cells encode alone,
 * rows join with CRLF), so joining chunk outputs reproduces the single-call
 * bytes exactly. Size checks measure the exact serialized protocol request
 * envelope; no guessed margin is subtracted.
 */
export const CSV_EXPORT_WASM_JSON_LIMIT_BYTES = 256 * 1024;

/** Split data rows into chunks that each fit one WASM encode request. */
export function chunkCsvRowsForExport(
  headers: readonly string[],
  dataRows: readonly (readonly string[])[],
  limitBytes: number = CSV_EXPORT_WASM_JSON_LIMIT_BYTES,
): readonly (readonly string[])[][] {
  const chunks: readonly (readonly string[])[][] = [];
  let current: (readonly string[])[] = [];
  for (const row of dataRows) {
    const candidate = [...current, row];
    // First invocation carries headers; later ones carry data rows only.
    // Measure the exact envelope that will be sent for this chunk.
    const invocationRows = chunks.length === 0
      ? [headers, ...candidate]
      : candidate;
    if (
      current.length > 0 &&
      spreadsheetCsvRequestBytes(invocationRows) > limitBytes
    ) {
      chunks.push(current);
      current = [row];
      continue;
    }
    current = candidate;
  }
  chunks.push(current);
  return chunks;
}

/** Encode all rows in bounded WASM requests and join with CRLF. */
export async function encodeSpreadsheetCsvChunked(
  headers: readonly string[],
  dataRows: readonly (readonly string[])[],
  limitBytes: number = CSV_EXPORT_WASM_JSON_LIMIT_BYTES,
): Promise<string> {
  const chunks = chunkCsvRowsForExport(headers, dataRows, limitBytes);
  const parts: string[] = [];
  for (const [index, chunk] of chunks.entries()) {
    const rows = index === 0 ? [headers, ...chunk] : chunk;
    parts.push(await encodeSpreadsheetCsv(rows));
  }
  return parts.join("\r\n");
}

export function FormTable(props: FormTableProps) {
  let sortMenuRef: HTMLDivElement | undefined;
  let filterToggleRef: HTMLButtonElement | undefined;

  const handleFilterToggle = () => {
    const hiding = showColumnFilters();
    // Capture focus before toggling: hiding detaches the focused input and
    // drops focus to the body, so the decision must use the pre-toggle tree.
    const active = hiding
      ? (document.activeElement as HTMLElement | null)
      : null;
    const inPanels = active?.closest?.(
      "#form-table-mobile-filters, #form-table-desktop",
    );
    const returnFocus = hiding && inPanels && active !== filterToggleRef;
    setShowColumnFilters((value) => !value);
    // When the panels close under a focused filter input, return focus to
    // the toggle so keyboard users are not stranded on detached content.
    // Removing the unfocused panel never moves focus away again.
    if (returnFocus) {
      filterToggleRef?.focus();
    }
  };
  // State for filtering and sorting
  const [globalFilter, setGlobalFilter] = createSignal("");
  const [sortField, setSortField] = createSignal<string | null>(null);
  const [sortDirection, setSortDirection] = createSignal<SortDirection>(null);
  const [columnFilters, setColumnFilters] = createSignal<
    Record<string, string>
  >({});
  const [showColumnFilters, setShowColumnFilters] = createSignal(true);
  const [showSortMenu, setShowSortMenu] = createSignal(false);
  const [isEditMode, setIsEditMode] = createSignal(false);
  const [editingCell, setEditingCell] = createSignal<
    { id: string; field: string } | null
  >(null);
  // Inline table action failures (export, cell update). No window.alert on
  // the product surface: the message renders here with a dismiss action.
  const [tableError, setTableError] = createSignal<string | null>(null);

  const canEditField = (field: string, value: unknown): boolean => {
    const type = props.entryForm.fields?.[field]?.type?.toLowerCase() ?? "";
    const structured = type === "list" || type.includes("asset") ||
      type.includes("object") || type.includes("relation");
    return !structured && isPlainEditableValue(value);
  };

  const [entries, { refetch }] = createResource(
    () => {
      /* v8 ignore start */
      if (!props.spaceId || !props.entryForm?.name) return false;
      /* v8 ignore stop */
      return { id: props.spaceId, formName: props.entryForm.name };
    },
    async ({ id, formName }) => {
      return await searchApi.query(id, { form: formName });
    },
  );

  const fields = createMemo(
    () =>
      /* v8 ignore start */
      props.entryForm?.fields ? Object.keys(props.entryForm.fields) : [],
    /* v8 ignore stop */
  );

  const sortableFields = createMemo(() => ["id", ...fields(), "updated_at"]);

  const processedEntries = createMemo(() => {
    const currentEntries = entries();
    if (!currentEntries) return [] as EntryRecord[];

    // 1. Global Filter
    let result = filterEntries([...currentEntries], fields(), globalFilter());

    // 2. Column Filters
    result = applyColumnFilters(result, columnFilters());

    // 3. Sorting
    return sortEntries(result, sortField() || "", sortDirection());
  });

  const handleHeaderClick = (field: string) => {
    if (sortField() === field) {
      if (sortDirection() === "asc") setSortDirection("desc");
      else setSortDirection(null);
    } else {
      setSortField(field);
      setSortDirection("asc");
    }
  };

  const handleSortFieldChange = (value: string) => {
    if (!value) {
      setSortField(null);
      setSortDirection(null);
      return;
    }
    setSortField(value);
    /* v8 ignore start */
    if (!sortDirection()) setSortDirection("asc");
    /* v8 ignore stop */
  };

  /* v8 ignore start */
  const handleSortMenuPointer = (event: PointerEvent) => {
    if (!showSortMenu()) return;
    if (!sortMenuRef || sortMenuRef.contains(event.target as Node)) return;
    setShowSortMenu(false);
  };

  const handleSortMenuKeydown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      setShowSortMenu(false);
    }
  };
  /* v8 ignore stop */

  onMount(() => {
    /* v8 ignore start */
    if (typeof document === "undefined") return;
    /* v8 ignore stop */
    document.addEventListener("pointerdown", handleSortMenuPointer);
    document.addEventListener("keydown", handleSortMenuKeydown);
  });

  onCleanup(() => {
    /* v8 ignore start */
    if (typeof document === "undefined") return;
    /* v8 ignore stop */
    document.removeEventListener("pointerdown", handleSortMenuPointer);
    document.removeEventListener("keydown", handleSortMenuKeydown);
  });

  const updateColumnFilter = (field: string, value: string) => {
    setColumnFilters((prev) => ({ ...prev, [field]: value }));
  };

  const downloadCSV = async () => {
    // Use untrack and try-catch for robustness in handler
    try {
      const { data, fieldNames, formName } = untrack(() => ({
        data: processedEntries(),
        fieldNames: fields(),
        /* v8 ignore start */
        formName: props.entryForm?.name || "export",
        /* v8 ignore stop */
      }));

      const headers = ["id", ...fieldNames, "updated_at"];
      const dataRows = data.map((entry) => formatCsvValues(entry, headers));
      /* v8 ignore start */
      const csvContent = await encodeSpreadsheetCsvChunked(
        headers,
        dataRows,
      );
      /* v8 ignore stop */

      const blob = new Blob([csvContent], { type: "text/csv;charset=utf-8;" });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.setAttribute("href", url);
      link.setAttribute("download", `${formName}_export.csv`);
      document.body.appendChild(link);
      link.click();
      document.body.removeChild(link);
      // Clean up the URL object after some time
      setTimeout(() => URL.revokeObjectURL(url), 100);
    } catch (err) {
      /* v8 ignore start */
      // biome-ignore lint/suspicious/noConsole: error reporting
      console.error("CSV Export failed:", err);
      setTableError(t("formTable.exportFailed"));
      /* v8 ignore stop */
    }
  };

  const handleCellUpdate = async (
    entryId: string,
    field: string,
    value: string,
  ) => {
    try {
      const currentRow = entries()?.find((item) => item.id === entryId);

      // Fetch full entry to get content and revision_id
      const entry = await entryApi.get(props.spaceId, entryId);
      let updatedMarkdown = entry.content;

      {
        /* v8 ignore start */
        const currentValue = formatValueForInput(
          currentRow?.properties?.[field],
        );
        if (currentValue === value) return;
        /* v8 ignore stop */
        updatedMarkdown = updateH2Section(updatedMarkdown, field, value);
      }

      const updatedEntry = await entryApi.update(props.spaceId, entryId, {
        markdown: updatedMarkdown,
        parent_revision_id: entry.revision_id,
      });
      void updatedEntry;
      refetch();
    } catch (err) {
      /* v8 ignore start */
      // biome-ignore lint/suspicious/noConsole: error logging
      console.error(t("formTable.updateFailed"), err);
      setTableError(
        `${t("formTable.updateFailed")}: ${
          err instanceof Error ? err.message : String(err)
        }`,
      );
      /* v8 ignore stop */
    }
  };

  // Determine if a cell is being edited
  const isCellEditing = (id: string, field: string) =>
    isEditMode() && editingCell()?.id === id && editingCell()?.field === field;

  // --- Drag Selection Logic ---
  const [selection, setSelection] = createSignal<{
    start: { r: number; c: number } | null;
    end: { r: number; c: number } | null;
  }>({ start: null, end: null });
  const [isSelecting, setIsSelecting] = createSignal(false);
  const handleGlobalMouseUp = () => setIsSelecting(false);
  const handleGlobalKeyDown = async (e: KeyboardEvent) => {
    /* v8 ignore start */
    if ((e.ctrlKey || e.metaKey) && e.key === "c") {
      // If focus is in an input or textarea, let the default copy behavior handle it
      const active = document.activeElement;
      if (
        active && (active.tagName === "INPUT" || active.tagName === "TEXTAREA")
      ) {
        return;
      }
      e.preventDefault();
      await copySelection();
    }
    /* v8 ignore stop */
  };

  onMount(() => {
    document.addEventListener("mouseup", handleGlobalMouseUp);
    document.addEventListener("keydown", handleGlobalKeyDown);
  });

  onCleanup(() => {
    document.removeEventListener("mouseup", handleGlobalMouseUp);
    document.removeEventListener("keydown", handleGlobalKeyDown);
  });

  const getRowData = (
    entry: EntryRecord,
    currentFields: string[],
    c1: number,
    c2: number,
  ) => {
    const rowData = [];
    // Col 0: stable Entry ID (identity, never a synthesized title)
    /* v8 ignore start */
    if (c1 <= 0 && c2 >= 0) rowData.push(entry.id);
    /* v8 ignore stop */

    // Cols 1..N: Fields
    for (let i = 0; i < currentFields.length; i++) {
      const colIdx = i + 1;
      /* v8 ignore start */
      if (colIdx >= c1 && colIdx <= c2) {
        rowData.push(
          formatValueForDisplay(
            entry.properties?.[currentFields[i]],
            "en-US",
            "",
          ),
        );
      }
      /* v8 ignore stop */
    }

    // Col N+1: Updated
    const lastCol = currentFields.length + 1;
    if (c1 <= lastCol && c2 >= lastCol) {
      rowData.push(formatDateLabel(entry.updated_at));
    }
    return rowData.join("\t");
  };

  const copySelection = async () => {
    const sel = selection();
    /* v8 ignore start */
    if (!sel.start || !sel.end || editingCell()) return;
    /* v8 ignore stop */

    const r1 = Math.min(sel.start.r, sel.end.r);
    const r2 = Math.max(sel.start.r, sel.end.r);
    const c1 = Math.min(sel.start.c, sel.end.c);
    const c2 = Math.max(sel.start.c, sel.end.c);

    const currentEntries = processedEntries();
    const currentFields = fields();

    const rowsData = [];
    for (let r = r1; r <= r2; r++) {
      const entry = currentEntries[r];
      /* v8 ignore start */
      if (!entry) continue;
      /* v8 ignore stop */
      rowsData.push(getRowData(entry, currentFields, c1, c2));
    }

    try {
      await navigator.clipboard.writeText(rowsData.join("\n"));
    } catch (err) {
      /* v8 ignore start */
      // biome-ignore lint/suspicious/noConsole: debugging
      console.error(t("common.copy"), err);
      /* v8 ignore stop */
    }
  };

  const handleCellMouseDown = (r: number, c: number) => {
    // Only set start/end, don't set isSelecting yet to allow text selection
    setSelection({ start: { r, c }, end: { r, c } });
  };

  const handleCellMouseEnter = (e: MouseEvent, r: number, c: number) => {
    /* v8 ignore start */
    if (isSelecting()) {
      setSelection((prev) => ({ ...prev, end: { r, c } }));
    } else if (selection().start && e.buttons === 1) {
      // Start drag selection if moving between cells with button down
      setIsSelecting(true);
      setSelection((prev) => ({ ...prev, end: { r, c } }));
    }
    /* v8 ignore stop */
  };

  const isSelected = (r: number, c: number) => {
    const sel = selection();
    if (!sel.start || !sel.end) return false;
    const r1 = Math.min(sel.start.r, sel.end.r);
    const r2 = Math.max(sel.start.r, sel.end.r);
    const c1 = Math.min(sel.start.c, sel.end.c);
    const c2 = Math.max(sel.start.c, sel.end.c);
    return r >= r1 && r <= r2 && c >= c1 && c <= c2;
  };

  /* v8 ignore start */
  return (
    <div
      class={`flex-1 h-full overflow-auto ui-surface ${
        isSelecting() ? "select-none" : ""
      }`}
    >
      <div class="p-4 sm:p-6">
        <div class="formTableToolbar mb-4 sm:mb-6 flex flex-wrap justify-between items-start gap-3">
          <div
            class="flex flex-wrap items-center gap-2"
            aria-busy={entries.loading || undefined}
          >
            <p class="ui-muted text-sm">
              {entries.error
                ? t("formTable.recordsError")
                : t("formTable.recordsFound", {
                  count: processedEntries().length,
                })}
            </p>
            {/* Inline spinner: table rows stay mounted during refetch. */}
            <Show when={entries.loading}>
              <LocalBusyIndicator
                size="sm"
                label={t("formTable.loading")}
              />
            </Show>
          </div>
          <div class="formTableActions flex flex-wrap justify-end gap-2">
            <button
              type="button"
              onClick={downloadCSV}
              class="ui-button ui-button-secondary text-sm flex items-center gap-2"
            >
              <svg
                class="w-4 h-4"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <title>{t("formTable.downloadCsv")}</title>
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4"
                />
              </svg>
              {t("formTable.exportCsv")}
            </button>
            <button
              type="button"
              onClick={() => setIsEditMode(!isEditMode())}
              class={`ui-button text-sm flex items-center gap-2 ${
                isEditMode() ? "ui-button-primary" : "ui-button-secondary"
              }`}
              title={isEditMode()
                ? t("formTable.disableEditing")
                : t("formTable.enableEditing")}
            >
              {isEditMode()
                ? (
                  <svg
                    class="w-4 h-4"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <title>{t("formTable.unlocked")}</title>
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      stroke-width="2"
                      d="M13.5 10.5V6.75a4.5 4.5 0 1 1 9 0v3.75M3.75 21.75h10.5a2.25 2.25 0 0 0 2.25-2.25v-6.75a2.25 2.25 0 0 0-2.25-2.25H3.75a2.25 2.25 0 0 0-2.25 2.25v6.75a2.25 2.25 0 0 0 2.25 2.25Z"
                    />
                  </svg>
                )
                : (
                  <svg
                    class="w-4 h-4"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <title>{t("formTable.locked")}</title>
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      stroke-width="2"
                      d="M16.5 10.5V6.75a4.5 4.5 0 1 0-9 0v3.75m-.75 11.25h10.5a2.25 2.25 0 0 0 2.25-2.25v-6.75a2.25 2.25 0 0 0-2.25-2.25H6.75a2.25 2.25 0 0 0-2.25 2.25v6.75a2.25 2.25 0 0 0 2.25 2.25Z"
                    />
                  </svg>
                )}
              {isEditMode() ? t("formTable.editable") : t("formTable.locked")}
            </button>
            <Show when={isEditMode() && props.onAddRow}>
              <button
                type="button"
                onClick={() => props.onAddRow?.()}
                class="ui-button ui-button-primary text-sm flex items-center gap-2"
              >
                <svg
                  class="w-4 h-4"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    d="M12 4v16m8-8H4"
                  />
                </svg>
                {t("formTable.addRow")}
              </button>
            </Show>
          </div>
        </div>

        <Show when={tableError()}>
          <div
            class="ui-alert ui-alert-error flex flex-wrap items-center justify-between gap-3"
            role="alert"
          >
            <span>{tableError()}</span>
            <button
              class="btn"
              type="button"
              aria-label={t("common.close")}
              onClick={() => setTableError(null)}
            >
              {t("common.close")}
            </button>
          </div>
        </Show>

        <Show when={entries.error}>
          <div
            class="ui-alert ui-alert-error flex flex-wrap items-center justify-between gap-3"
            role="alert"
          >
            <span>{t("formTable.loadRecordsError")}</span>
            <button class="btn" type="button" onClick={() => void refetch()}>
              {t("common.retry")}
            </button>
          </div>
        </Show>

        <div class="mb-4 ui-card ui-stack-sm">
          <div class="flex flex-wrap items-center gap-2 justify-between">
            <input
              type="text"
              placeholder={t("formTable.globalSearch")}
              aria-label={t("formTable.globalSearch")}
              class="ui-input w-full max-w-md"
              value={globalFilter()}
              onInput={(e) => setGlobalFilter(e.currentTarget.value)}
            />
            <div class="flex flex-wrap gap-2">
              <div
                class="ui-menu"
                ref={(el) => {
                  sortMenuRef = el;
                }}
              >
                <button
                  type="button"
                  class="ui-button ui-button-secondary text-sm"
                  onClick={() => setShowSortMenu((value) => !value)}
                  aria-label={t("formTable.sortMenu")}
                  aria-expanded={showSortMenu()}
                >
                  {t("formTable.sort")}
                </button>
                <Show when={showSortMenu()}>
                  <div class="ui-menu-panel">
                    <div class="ui-menu-section">
                      <p class="ui-menu-title">{t("formTable.sortField")}</p>
                      <select
                        aria-label={t("formTable.sortField")}
                        class="ui-input"
                        value={sortField() ?? ""}
                        onChange={(e) =>
                          handleSortFieldChange(e.currentTarget.value)}
                      >
                        <option value="">{t("formTable.none")}</option>
                        <For each={sortableFields()}>
                          {(field) => <option value={field}>{field}</option>}
                        </For>
                      </select>
                    </div>
                    <div class="ui-menu-section">
                      <p class="ui-menu-title">{t("formTable.direction")}</p>
                      <div class="ui-menu-options">
                        <label class="ui-radio">
                          <input
                            type="radio"
                            name="sort-direction"
                            value="asc"
                            checked={sortDirection() === "asc"}
                            onChange={() => setSortDirection("asc")}
                          />
                          <span>{t("formTable.ascending")}</span>
                        </label>
                        <label class="ui-radio">
                          <input
                            type="radio"
                            name="sort-direction"
                            value="desc"
                            checked={sortDirection() === "desc"}
                            onChange={() => setSortDirection("desc")}
                          />
                          <span>{t("formTable.descending")}</span>
                        </label>
                      </div>
                    </div>
                  </div>
                </Show>
              </div>
              <button
                type="button"
                class={`ui-button text-sm ${
                  showColumnFilters()
                    ? "ui-button-primary"
                    : "ui-button-secondary"
                }`}
                ref={(el) => {
                  filterToggleRef = el;
                }}
                aria-expanded={showColumnFilters()}
                aria-controls="form-table-mobile-filters form-table-desktop"
                onClick={handleFilterToggle}
              >
                {t("formTable.filter")}
              </button>
            </div>
          </div>
        </div>

        <Show when={showColumnFilters()}>
          <div class="ui-table-mobile-filters" id="form-table-mobile-filters">
            <For each={["id", ...fields(), "updated_at"]}>
              {(field) => (
                <label class="ui-table-mobile-filter">
                  <span>
                    {field === "id"
                      ? t("formTable.id")
                      : field === "updated_at"
                      ? t("formTable.updated")
                      : field}
                  </span>
                  <input
                    type="text"
                    class="ui-input ui-input-sm"
                    placeholder={t("formTable.columnFilter")}
                    aria-label={`${field} ${t("formTable.columnFilter")}`}
                    value={columnFilters()[field] || ""}
                    onInput={(event) =>
                      updateColumnFilter(field, event.currentTarget.value)}
                  />
                </label>
              )}
            </For>
          </div>
        </Show>

        <div
          class="ui-table-wrapper ui-table-desktop overflow-x-auto"
          id="form-table-desktop"
        >
          <table class="ui-table">
            <thead class="ui-table-head">
              <tr>
                <th
                  scope="col"
                  class="ui-table-header-cell w-10 sticky top-0 z-10"
                  aria-label={t("formTable.actions")}
                >
                  <span class="sr-only">{t("formTable.actions")}</span>
                </th>
                <th scope="col" class="ui-table-header-cell sticky top-0 z-10">
                  <div class="flex flex-col gap-2">
                    <button
                      type="button"
                      class="ui-table-header-button select-none"
                      onClick={() => handleHeaderClick("id")}
                    >
                      {t("formTable.id")}
                      <SortIcon
                        active={sortField() === "id"}
                        direction={sortDirection()}
                      />
                    </button>
                    <Show when={showColumnFilters()}>
                      <input
                        type="text"
                        class="ui-input ui-input-sm ui-table-filter text-xs"
                        placeholder={t("formTable.columnFilter")}
                        aria-label={`${t("formTable.id")} ${t("formTable.columnFilter")}`}
                        value={columnFilters().id || ""}
                        onInput={(e) =>
                          updateColumnFilter("id", e.currentTarget.value)}
                        onClick={(e) => e.stopPropagation()}
                      />
                    </Show>
                  </div>
                </th>

                <For each={fields()}>
                  {(field) => (
                    <th
                      scope="col"
                      class="ui-table-header-cell sticky top-0 z-10"
                    >
                      <div class="flex flex-col gap-2">
                        <button
                          type="button"
                          class="ui-table-header-button select-none"
                          onClick={() => handleHeaderClick(field)}
                        >
                          {field}
                          <SortIcon
                            active={sortField() === field}
                            direction={sortDirection()}
                          />
                        </button>
                        <Show when={showColumnFilters()}>
                          <input
                            type="text"
                            class="ui-input ui-input-sm ui-table-filter text-xs"
                            placeholder={t("formTable.columnFilter")}
                            aria-label={`${field} ${t("formTable.columnFilter")}`}
                            value={columnFilters()[field] || ""}
                            onInput={(e) =>
                              updateColumnFilter(field, e.currentTarget.value)}
                            onClick={(e) => e.stopPropagation()}
                          />
                        </Show>
                      </div>
                    </th>
                  )}
                </For>

                <th scope="col" class="ui-table-header-cell sticky top-0 z-10">
                  <div class="flex flex-col gap-2">
                    <button
                      type="button"
                      class="ui-table-header-button select-none"
                      onClick={() => handleHeaderClick("updated_at")}
                    >
                      {t("formTable.updated")}
                      <SortIcon
                        active={sortField() === "updated_at"}
                        direction={sortDirection()}
                      />
                    </button>
                    <Show when={showColumnFilters()}>
                      <input
                        type="text"
                        class="ui-input ui-input-sm ui-table-filter text-xs"
                        placeholder={t("formTable.columnFilter")}
                        aria-label={`${t("formTable.updated")} ${t("formTable.columnFilter")}`}
                        value={columnFilters().updated_at || ""}
                        onInput={(e) =>
                          updateColumnFilter(
                            "updated_at",
                            e.currentTarget.value,
                          )}
                        onClick={(e) => e.stopPropagation()}
                      />
                    </Show>
                  </div>
                </th>
              </tr>
            </thead>
            <tbody class="ui-table-body">
              <For each={processedEntries()}>
                {(entry, rowIndex) => (
                  <tr class="ui-table-row">
                    <td class="ui-table-cell ui-table-cell-muted whitespace-nowrap">
                      <button
                        type="button"
                        onClick={() => props.onEntryClick(entry.id)}
                        class="ui-button ui-button-secondary ui-button-sm inline-flex items-center gap-2 text-xs"
                        title={t("formTable.viewEntry")}
                        aria-label={t("formTable.viewEntry")}
                      >
                        <svg
                          class="w-4 h-4"
                          fill="none"
                          stroke="currentColor"
                          viewBox="0 0 24 24"
                        >
                          <title>{t("formTable.viewEntry")}</title>
                          <path
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            stroke-width="2"
                            d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14"
                          />
                        </svg>
                        <span>{t("formTable.view")}</span>
                      </button>
                    </td>
                    {/* biome-ignore lint/a11y/useKeyWithClickEvents: drag select is mouse-only for now */}
                    <td
                      class={`ui-table-cell whitespace-nowrap font-medium ${
                        isSelected(rowIndex(), 0)
                          ? "ui-table-cell-selected"
                          : ""
                      }`}
                      onMouseDown={() => handleCellMouseDown(rowIndex(), 0)}
                      onMouseEnter={(e) =>
                        handleCellMouseEnter(e, rowIndex(), 0)}
                    >
                      {entry.id}
                    </td>
                    <For each={fields()}>
                      {(field, fieldIndex) => (
                        // biome-ignore lint/a11y/useKeyWithClickEvents: drag select is mouse-only for now
                        <td
                          class={`ui-table-cell ui-table-cell-muted whitespace-nowrap ${
                            isSelected(rowIndex(), fieldIndex() + 1)
                              ? "ui-table-cell-selected"
                              : ""
                          }`}
                          onMouseDown={() =>
                            handleCellMouseDown(rowIndex(), fieldIndex() + 1)}
                          onMouseEnter={(e) =>
                            handleCellMouseEnter(
                              e,
                              rowIndex(),
                              fieldIndex() + 1,
                            )}
                          onClick={(e) => {
                            if (
                              isEditMode() &&
                              canEditField(field, entry.properties?.[field])
                            ) {
                              e.stopPropagation();
                              setEditingCell({ id: entry.id, field });
                            }
                          }}
                        >
                          <Show
                            when={isCellEditing(entry.id, field)}
                            fallback={formatValueForDisplay(
                              entry.properties?.[field],
                            )}
                          >
                            <input
                              value={formatValueForInput(
                                entry.properties?.[field],
                              )}
                              onBlur={(e) => {
                                const newVal = e.currentTarget.value;
                                handleCellUpdate(entry.id, field, newVal);
                                if (
                                  editingCell()?.id === entry.id &&
                                  editingCell()?.field === field
                                ) {
                                  setEditingCell(null);
                                }
                              }}
                              onKeyDown={(e) =>
                                e.key === "Enter" && e.currentTarget.blur()}
                              class="ui-table-cell-input"
                              autofocus
                              onClick={(e) => e.stopPropagation()}
                            />
                          </Show>
                        </td>
                      )}
                    </For>
                    <td
                      class={`ui-table-cell ui-table-cell-muted whitespace-nowrap ${
                        isSelected(rowIndex(), fields().length + 1)
                          ? "ui-table-cell-selected"
                          : ""
                      }`}
                      onMouseDown={() =>
                        handleCellMouseDown(rowIndex(), fields().length + 1)}
                      onMouseEnter={(e) =>
                        handleCellMouseEnter(
                          e,
                          rowIndex(),
                          fields().length + 1,
                        )}
                    >
                      {formatDateLabel(entry.updated_at)}
                    </td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </div>
        <div
          class="ui-table-mobile-list"
          role="list"
          aria-label={t("formTable.mobileList")}
        >
          <For each={processedEntries()}>
            {(entry) => (
              <article class="ui-table-mobile-card" role="listitem">
                <div class="ui-table-mobile-card-header">
                  <button
                    type="button"
                    class="ui-table-mobile-title"
                    onClick={() => props.onEntryClick(entry.id)}
                  >
                    {entry.id}
                  </button>
                  <button
                    type="button"
                    class="ui-button ui-button-secondary ui-button-sm"
                    onClick={() => props.onEntryClick(entry.id)}
                    aria-label={t("formTable.viewEntry")}
                  >
                    {t("formTable.view")}
                  </button>
                </div>
                <dl class="ui-table-mobile-fields">
                  <For each={fields().slice(0, 3)}>
                    {(field) => (
                      <div class="ui-table-mobile-field">
                        <dt>{field}</dt>
                        <dd>
                          <Show
                            when={isCellEditing(entry.id, field)}
                            fallback={
                              <Show
                                when={isEditMode() &&
                                  canEditField(
                                    field,
                                    entry.properties?.[field],
                                  )}
                                fallback={
                                  <span class="ui-table-mobile-value">
                                    {formatValueForDisplay(
                                      entry.properties?.[field],
                                    )}
                                  </span>
                                }
                              >
                                <button
                                  type="button"
                                  class="ui-table-mobile-value"
                                  onClick={() =>
                                    setEditingCell({ id: entry.id, field })}
                                >
                                  {formatValueForDisplay(
                                    entry.properties?.[field],
                                  )}
                                </button>
                              </Show>
                            }
                          >
                            <input
                              value={formatValueForInput(
                                entry.properties?.[field],
                              )}
                              class="ui-table-cell-input"
                              autofocus
                              aria-label={field}
                              onBlur={(event) => {
                                void handleCellUpdate(
                                  entry.id,
                                  field,
                                  event.currentTarget.value,
                                );
                                setEditingCell(null);
                              }}
                              onKeyDown={(event) =>
                                event.key === "Enter" &&
                                event.currentTarget.blur()}
                            />
                          </Show>
                        </dd>
                      </div>
                    )}
                  </For>
                  <div class="ui-table-mobile-field">
                    <dt>{t("formTable.updated")}</dt>
                    <dd>{formatDateLabel(entry.updated_at)}</dd>
                  </div>
                </dl>
                <Show when={fields().length > 3}>
                  <details class="ui-table-mobile-more">
                    <summary>
                      {t(
                        fields().length - 3 === 1
                          ? "formTable.showMoreField"
                          : "formTable.showMoreFields",
                        {
                          count: fields().length - 3,
                        },
                      )}
                    </summary>
                    <dl class="ui-table-mobile-fields ui-table-mobile-extra-fields">
                      <For each={fields().slice(3)}>
                        {(field) => (
                          <div class="ui-table-mobile-field">
                            <dt>{field}</dt>
                            <dd>
                              <Show
                                when={isCellEditing(entry.id, field)}
                                fallback={
                                  <Show
                                    when={isEditMode() &&
                                      canEditField(
                                        field,
                                        entry.properties?.[field],
                                      )}
                                    fallback={
                                      <span class="ui-table-mobile-value">
                                        {formatValueForDisplay(
                                          entry.properties?.[field],
                                        )}
                                      </span>
                                    }
                                  >
                                    <button
                                      type="button"
                                      class="ui-table-mobile-value"
                                      onClick={() =>
                                        setEditingCell({ id: entry.id, field })}
                                    >
                                      {formatValueForDisplay(
                                        entry.properties?.[field],
                                      )}
                                    </button>
                                  </Show>
                                }
                              >
                                <input
                                  value={formatValueForInput(
                                    entry.properties?.[field],
                                  )}
                                  class="ui-table-cell-input"
                                  autofocus
                                  aria-label={field}
                                  onBlur={(event) => {
                                    void handleCellUpdate(
                                      entry.id,
                                      field,
                                      event.currentTarget.value,
                                    );
                                    setEditingCell(null);
                                  }}
                                  onKeyDown={(event) => event.key === "Enter" &&
                                    event.currentTarget.blur()}
                                />
                              </Show>
                            </dd>
                          </div>
                        )}
                      </For>
                    </dl>
                  </details>
                </Show>
              </article>
            )}
          </For>
        </div>
      </div>
    </div>
  );
}
/* v8 ignore stop */
