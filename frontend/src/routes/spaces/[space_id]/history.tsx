import { useLocation, useParams, useSearchParams } from "@solidjs/router";
import { createEffect, createMemo, createResource, createSignal, For, on, onCleanup, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { ConfirmDestructiveAction } from "~/components/ConfirmDestructiveAction";
import { IconButton } from "~/components/IconButton";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { ResultPagination } from "~/components/ResultPagination";
import { HistoryChangeTable, type HistoryChangeColumn } from "~/components/HistoryChangeTable";
import { formatDateTimeLabel } from "~/lib/date-format";
import { actorDisplayNameLookup, shortActorFallback } from "~/lib/entry-history";
import { t } from "~/lib/i18n";
import { changeApi, formApi, spaceApi, type SpaceChangeQueryRow, type SpaceChangeSort } from "~/lib/ugoite-client";
import type { Form, SpaceMember } from "~/lib/types";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "history" });

const PAGE_SIZE = 50;
const SORT_FIELDS: SpaceChangeSort["field"][] = [
  "created_at_micros",
  "actor_principal_id",
  "run_id",
];

const userValue = (value: { state: string; value?: unknown }): string => {
  if (value.state === "missing") return t("spaceHistory.emptyValue");
  if (value.state !== "value") return t("spaceHistory.unavailableValue");
  if (typeof value.value === "string") return value.value;
  if (typeof value.value === "number" || typeof value.value === "boolean") {
    return String(value.value);
  }
  return t("spaceHistory.unavailableValue");
};

export default function SpaceHistoryRoute() {
  const params = useParams<{ space_id: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  const location = useLocation<{ historyRecoveryNotice?: unknown }>();
  const spaceId = () => params.space_id;
  const [text, setText] = createSignal("");
  const [actorId, setActorId] = createSignal("");
  const [afterDate, setAfterDate] = createSignal("");
  const [beforeDate, setBeforeDate] = createSignal("");
  const [sort, setSort] = createSignal<SpaceChangeSort[]>([]);
  const [visible, setVisible] = createSignal({
    operation: true,
    actor: true,
    date: true,
  });
  const [cursors, setCursors] = createSignal<(string | undefined)[]>([undefined]);
  const [selectedChange, setSelectedChange] = createSignal<string>();
  const [openChange, setOpenChange] = createSignal<string>();
  const [targetRows, setTargetRows] = createSignal<NonNullable<Awaited<ReturnType<typeof changeApi.inspect>>>["targets"]>([]);
  const [targetCursor, setTargetCursor] = createSignal<string>();
  const [targetHistory, setTargetHistory] = createSignal<(string | undefined)[]>([undefined]);
  const [targetLoading, setTargetLoading] = createSignal(false);
  const [targetError, setTargetError] = createSignal(false);
  const [selectedEntryId, setSelectedEntryId] = createSignal<string>();
  const [relatedChanges, setRelatedChanges] = createSignal<SpaceChangeQueryRow[]>([]);
  const [relatedCursor, setRelatedCursor] = createSignal<string>();
  const [relatedHistory, setRelatedHistory] = createSignal<(string | undefined)[]>([undefined]);
  const [relatedLoading, setRelatedLoading] = createSignal(false);
  const [pendingRecovery, setPendingRecovery] = createSignal<"revert" | "undo" | null>(null);
  const [recoveryMessage, setRecoveryMessage] = createSignal("");
  const [recoveryBusy, setRecoveryBusy] = createSignal(false);
  const initialRecoveryNotice = location.state?.historyRecoveryNotice;
  const [recoveryNotice, setRecoveryNotice] = createSignal<string | null>(
    typeof initialRecoveryNotice === "string" ? initialRecoveryNotice : null,
  );
  const [recoveryFailure, setRecoveryFailure] = createSignal<string | null>(null);
  const [historyNeedsReview, setHistoryNeedsReview] = createSignal(false);
  const [recoveryAttempt, setRecoveryAttempt] = createSignal<{
    kind: "revert" | "undo";
    changeId: string;
    runId: string | null;
  } | null>(null);
  let detailOpener: HTMLButtonElement | undefined;
  let detailCloseButton: HTMLButtonElement | undefined;
  const [inspection] = createResource(openChange, (changeId) =>
    changeId ? changeApi.inspect(spaceId(), changeId, { limit: 10 }) : undefined
  );
  const [affectedEntry] = createResource(
    () => openChange() && selectedEntryId() ? `${openChange()}\n${selectedEntryId()}` : undefined,
    async (key) => {
      const [changeId, entryId] = key.split("\n");
      return await changeApi.affectedEntry(spaceId(), changeId, entryId);
    },
  );
  const cursor = () => cursors()[cursors().length - 1];
  const queryKey = createMemo(() => JSON.stringify({
    spaceId: spaceId(),
    text: text().trim() || undefined,
    actor: actorId() || undefined,
    after: afterDate() || undefined,
    before: beforeDate() || undefined,
    sort: sort(),
    cursor: cursor(),
  }));
  const [page, { refetch }] = createResource(queryKey, async (key) => {
    const query = JSON.parse(key) as {
      spaceId: string;
      text?: string;
      actor?: string;
      after?: string;
      before?: string;
      sort: SpaceChangeSort[];
      cursor?: string;
    };
    const micros = (date: string, end: boolean) => {
      if (!date) return undefined;
      const parsed = Date.parse(`${date}T${end ? "23:59:59.999" : "00:00:00.000"}Z`);
      return Number.isFinite(parsed)
        ? parsed * 1000 + (end ? 999 : 0)
        : undefined;
    };
    return await changeApi.query(query.spaceId, {
      limit: PAGE_SIZE,
      cursor: query.cursor,
      text: query.text,
      actor_principal_id: query.actor,
      created_after_micros: micros(query.after ?? "", false),
      created_before_micros: micros(query.before ?? "", true),
      sort: query.sort,
    });
  });
  const [members] = createResource(spaceId, (id): Promise<SpaceMember[]> =>
    spaceApi.listMembers(id).catch(() => [])
  );
  const [forms] = createResource(spaceId, (id): Promise<Form[]> =>
    formApi.list(id).catch(() => [])
  );
  const actorLookup = createMemo(() => actorDisplayNameLookup(
    (members() ?? []).map((member) => ({
      principal_id: member.principal.principal_id,
      display_name: member.principal.display_name,
    })),
  ));
  const actorName = (id: string) =>
    actorLookup()?.(id)?.trim() || shortActorFallback(id);
  const actorOptions = () => [...new Set([
    ...(members() ?? []).map((member) => member.principal.principal_id),
    ...currentRows().map((row) => row.change.actor_principal_id),
  ])].filter(Boolean);
  const formName = (id: string) => {
    const form = (forms() ?? []).find((candidate) =>
      candidate.id === id || candidate.name === id
    );
    return form?.name ?? t("spaceHistory.unknownTarget");
  };
  const fieldName = (formId: string, fieldId: string) => {
    const form = (forms() ?? []).find((candidate) =>
      candidate.id === formId || candidate.name === formId
    );
    const field = Object.entries(form?.fields ?? {}).find(([name, value]) =>
      name === fieldId || value.id === fieldId
    );
    return field?.[0] ?? t("spaceHistory.unknownField");
  };
  const showSummary = (row: SpaceChangeQueryRow) => {
    if (row.target_visibility !== "complete" || !row.summary) {
      return t("spaceHistory.restrictedSummary");
    }
    const [first, ...rest] = row.summary.field_groups;
    if (!first) return t("spaceHistory.noFieldChanges");
    const groupLabel = `${fieldName(first.form_id, first.field_id)}: ${userValue(first.before)} → ${userValue(first.after)}${first.affected_entry_count > 1 ? ` (${first.affected_entry_count})` : ""}`;
    return rest.length
      ? `${groupLabel}, ${t("spaceHistory.moreGroups", { count: rest.length })}`
      : groupLabel;
  };
  const targetLabel = (row: SpaceChangeQueryRow) => {
    if (row.target_visibility !== "complete" || !row.summary) {
      return t("spaceHistory.restrictedTarget");
    }
    const formIds = row.summary.target_form_ids;
    const targetName = formIds.length === 1
      ? formName(formIds[0])
      : formIds.length > 1
      ? t("spaceHistory.targetForms", { count: formIds.length })
      : t("spaceHistory.unknownTarget");
    return `${targetName} · ${t("spaceHistory.targetCount", { count: row.summary.affected_entry_count })}`;
  };
  const changeTitle = (row: SpaceChangeQueryRow) =>
    row.change.message?.trim() || targetLabel(row);
  const openedRow = () => currentRows().find((row) => row.change_id === openChange());
  const detailRow = () => {
    const row = openedRow();
    const inspected = inspection();
    if (row || !inspected) return row;
    return {
      change_id: inspected.change_id,
      generation: 0,
      change: inspected.change,
      publication: { generation: 0, publication_uri: { space_uid: "", key: "" }, publication_checksum: "" },
      target_visibility: inspected.target_visibility,
      summary: inspected.summary,
    } satisfies SpaceChangeQueryRow;
  };
  const setDetail = (changeId?: string, navigationOptions?: { state?: unknown }) => {
    if (changeId) {
      setOpenChange(changeId);
      setSearchParams({ change: changeId });
    } else {
      setOpenChange(undefined);
      setSelectedEntryId(undefined);
      if (navigationOptions) setSearchParams({ change: undefined }, navigationOptions);
      else setSearchParams({ change: undefined });
    }
  };
  createEffect(on(
    () => searchParams.change,
    (value) => setOpenChange(typeof value === "string" && value ? value : undefined),
  ));
  createEffect(() => {
    const current = inspection();
    if (!current || current.change_id !== openChange()) return;
    setTargetRows(current.targets);
    setTargetCursor(current.next_cursor ?? undefined);
    setTargetHistory([undefined]);
    setTargetError(false);
    setSelectedEntryId(current.targets[0]?.entry_id);
    setRelatedChanges([]);
    setRelatedCursor(undefined);
    setRelatedHistory([undefined]);
    if (current.change.run_id) {
      const runId = current.change.run_id;
      void changeApi.query(spaceId(), { limit: 10, run_id: runId })
        .then((page) => {
          if (openChange() !== current.change_id) return;
          setRelatedChanges(page.changes);
          setRelatedCursor(page.next_cursor ?? undefined);
        })
        .catch(() => { if (openChange() === current.change_id) setRelatedChanges([]); });
    }
  });
  const loadTargetPage = async (cursor: string | undefined, direction: "next" | "previous") => {
    const changeId = openChange();
    if (!changeId || targetLoading()) return;
    setTargetLoading(true);
    setTargetError(false);
    try {
      const next = await changeApi.inspect(spaceId(), changeId, { limit: 10, cursor });
      if (openChange() !== changeId) return;
      setTargetRows(next.targets);
      setSelectedEntryId(next.targets[0]?.entry_id);
      setTargetCursor(next.next_cursor ?? undefined);
      setTargetHistory((current) => direction === "next"
        ? [...current, cursor]
        : current.slice(0, -1));
    } catch {
      setTargetError(true);
    } finally {
      setTargetLoading(false);
    }
  };
  const loadRelatedPage = async (cursor: string | undefined, direction: "next" | "previous") => {
    const runId = inspection()?.change.run_id;
    const changeId = openChange();
    if (!runId || !changeId || relatedLoading()) return;
    setRelatedLoading(true);
    try {
      const next = await changeApi.query(spaceId(), { limit: 10, run_id: runId, cursor });
      if (openChange() !== changeId) return;
      setRelatedChanges(next.changes);
      setRelatedCursor(next.next_cursor ?? undefined);
      setRelatedHistory((current) => direction === "next"
        ? [...current, cursor]
        : current.slice(0, -1));
    } catch {
      // The initial Change detail remains available if related history cannot be read.
    } finally {
      setRelatedLoading(false);
    }
  };
  const closeDetail = (navigationOptions?: { state?: unknown }) => {
    setDetail(undefined, navigationOptions);
    queueMicrotask(() => detailOpener?.focus());
  };
  createEffect(() => {
    if (!openChange()) return;
    const appRoot = document.getElementById("app");
    appRoot?.setAttribute("inert", "");
    queueMicrotask(() => detailCloseButton?.focus());
    onCleanup(() => {
      appRoot?.removeAttribute("inert");
      queueMicrotask(() => {
        if (detailOpener?.isConnected) detailOpener.focus();
      });
    });
  });
  const handleDetailKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      closeDetail();
      return;
    }
    if (event.key !== "Tab") return;
    const dialog = event.currentTarget as HTMLElement;
    const focusable = [...dialog.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), [href], [tabindex]:not([tabindex="-1"])',
    )];
    if (focusable.length === 0) {
      event.preventDefault();
      return;
    }
    const index = focusable.indexOf(document.activeElement as HTMLElement);
    if (event.shiftKey && index <= 0) {
      event.preventDefault();
      focusable.at(-1)?.focus();
    } else if (!event.shiftKey && (index < 0 || index === focusable.length - 1)) {
      event.preventDefault();
      focusable[0].focus();
    }
  };
  const refreshHistory = async (failureKey: "spaceHistory.refreshFailedAfterConflict" | "spaceHistory.refreshFailedAfterUnknownResult") => {
    setHistoryNeedsReview(true);
    try {
      await refetch();
      if (page.error) throw page.error;
      setHistoryNeedsReview(false);
    } catch {
      setRecoveryFailure(t(failureKey));
    }
  };
  const reconcileRecovery = async (attempt = recoveryAttempt()) => {
    if (!attempt) return;
    setHistoryNeedsReview(true);
    try {
      const changes: SpaceChangeQueryRow[] = [];
      let cursor: string | undefined;
      let complete = false;
      for (let index = 0; index < 5; index += 1) {
        const page = await changeApi.query(spaceId(), {
          limit: PAGE_SIZE,
          cursor,
          ...(attempt.kind === "undo" && attempt.runId
            ? { run_id: attempt.runId }
            : {}),
        });
        changes.push(...page.changes);
        if (!page.next_cursor) {
          complete = true;
          break;
        }
        cursor = page.next_cursor;
      }
      let confirmed = false;
      let safeToRetry = false;
      if (attempt.kind === "revert") {
        confirmed = changes.some((change) =>
          change.change.reverts_change_id === attempt.changeId
        );
        safeToRetry = !confirmed && complete;
      } else {
        const originals = changes.filter((change) =>
          change.change.run_id === attempt.runId && !change.change.reverts_change_id
        );
        const reverted = new Set(changes.flatMap((change) =>
          change.change.run_id === attempt.runId && change.change.reverts_change_id
            ? [change.change.reverts_change_id]
            : []
        ));
        confirmed = originals.length > 0 && originals.every((change) =>
          reverted.has(change.change_id)
        );
        safeToRetry = complete && !confirmed;
      }
      if (!confirmed && !safeToRetry) {
        setRecoveryFailure(t("spaceHistory.operationResultUnknown"));
        return;
      }
      setHistoryNeedsReview(false);
      setRecoveryAttempt(null);
      if (confirmed) {
        setRecoveryFailure(null);
        setRecoveryNotice(t("spaceHistory.operationResultConfirmed"));
      } else if (attempt.kind === "undo" && changes.some((change) =>
        change.change.run_id === attempt.runId && change.change.reverts_change_id
      )) {
        setRecoveryFailure(t("spaceHistory.runUndoPartiallyCommitted"));
      } else {
        setRecoveryFailure(t("spaceHistory.operationNotCommitted"));
      }
      try {
        await refetch();
      } catch {
        // The independent reconciliation read is authoritative for recovery safety.
      }
    } catch {
      setRecoveryFailure(t("spaceHistory.refreshFailedAfterUnknownResult"));
    }
  };
  const isDefiniteRevisionConflict = (error: unknown): boolean => {
    if (!error || typeof error !== "object") return false;
    const diagnostic = error as { code?: unknown; status?: unknown };
    return diagnostic.code === "REVISION_CONFLICT" && diagnostic.status === 409;
  };
  const isDefiniteApiRejection = (error: unknown): boolean => {
    if (!error || typeof error !== "object") return false;
    const diagnostic = error as { kind?: unknown; status?: unknown; mutationOutcome?: unknown };
    if (diagnostic.mutationOutcome) return diagnostic.mutationOutcome === "rejected";
    return typeof diagnostic.kind === "string" && typeof diagnostic.status === "number" && diagnostic.status >= 400 && diagnostic.status < 500 && diagnostic.status !== 408 && diagnostic.status !== 425;
  };
  const confirmRecovery = async () => {
    const row = detailRow();
    const kind = pendingRecovery();
    if (!row || !kind || recoveryBusy()) return;
    setRecoveryBusy(true);
    const attempt = {
      kind,
      changeId: row.change_id,
      runId: row.change.run_id,
    };
    let successNotice: string | null = null;
    try {
      if (kind === "revert") {
        await changeApi.revert(spaceId(), row.change_id, recoveryMessage().trim() ? { message: recoveryMessage().trim() } : {});
        successNotice = t("spaceHistory.revertSuccess");
      } else if (row.change.run_id) {
        const result = await changeApi.undoRun(spaceId(), row.change.run_id);
        successNotice = t("spaceHistory.undoSuccess", { count: result.reverted_change_count });
      }
      setPendingRecovery(null);
      setRecoveryMessage("");
      setRecoveryFailure(null);
      setHistoryNeedsReview(true);
      try {
        await refetch();
        if (page.error) throw page.error;
        setHistoryNeedsReview(false);
      } catch {
        setRecoveryFailure(t("spaceHistory.refreshFailedAfterSave"));
      }
      if (successNotice) {
        setRecoveryNotice(successNotice);
        closeDetail({ state: { historyRecoveryNotice: successNotice } });
      } else {
        closeDetail();
      }
    } catch (error) {
      if (isDefiniteRevisionConflict(error)) {
        setPendingRecovery(null);
        closeDetail();
        setRecoveryFailure(t("spaceHistory.conflict"));
        await refreshHistory("spaceHistory.refreshFailedAfterConflict");
      } else if (isDefiniteApiRejection(error)) {
        setPendingRecovery(null);
        closeDetail();
        setRecoveryFailure(t("spaceHistory.operationFailed"));
      } else {
        setPendingRecovery(null);
        closeDetail();
        setRecoveryAttempt(attempt);
        setRecoveryFailure(t("spaceHistory.operationResultUnknown"));
        await reconcileRecovery(attempt);
      }
    } finally {
      setRecoveryBusy(false);
    }
  };
  const columns = createMemo((): HistoryChangeColumn[] => {
    const result: HistoryChangeColumn[] = [
      { key: "target", label: t("spaceHistory.target"), cell: (row) => <span title={targetLabel(row)}>{targetLabel(row)}</span> },
      { key: "summary", label: t("spaceHistory.summary"), cell: (row) => <span title={showSummary(row)}>{showSummary(row)}</span> },
    ];
    if (visible().operation) result.push({
      key: "operation",
      label: t("spaceHistory.operation"),
      cell: (row) => row.change.reverts_change_id
        ? t("spaceHistory.revert")
        : t("spaceHistory.update"),
    });
    if (visible().actor) result.push({
      key: "actor",
      label: t("spaceHistory.actor"),
      cell: (row) => actorName(row.change.actor_principal_id),
    });
    if (visible().date) result.push({
      key: "date",
      label: t("spaceHistory.date"),
      cell: (row) => formatDateTimeLabel(row.change.created_at_micros / 1000),
    });
    return result;
  });
  const resetPaging = () => {
    setCursors([undefined]);
    setSelectedChange(undefined);
  };
  const updateSort = (field: SpaceChangeSort["field"], direction: "asc" | "desc") => {
    setSort((current) => {
      const remaining = current.filter((item) => item.field !== field);
      return [...remaining, { field, direction }].slice(-3);
    });
    resetPaging();
  };
  const currentRows = () => page()?.changes ?? [];

  return (
    <section class="space-history" aria-busy={page.loading || undefined}>
      <h1>{t("spaceHistory.title")}</h1>
      <div class="entry-browser-toolbar" role="toolbar" aria-label={t("spaceHistory.toolbar")}>
        <label class="entry-browser-search">
          <span class="ui-sr-only">{t("spaceHistory.search")}</span>
          <input class="ui-input" type="search" value={text()} placeholder={t("spaceHistory.searchPlaceholder")} onInput={(event) => { setText(event.currentTarget.value); resetPaging(); }} />
        </label>
        <details class="history-controls">
          <summary>{t("spaceHistory.filtersAndDisplay")}</summary>
          <div class="history-controls-panel">
            <fieldset>
              <legend>{t("spaceHistory.columns")}</legend>
              <For each={Object.keys(visible()) as Array<keyof ReturnType<typeof visible>>}>{(key) => <label><input type="checkbox" checked={visible()[key]} onChange={(event) => setVisible((current) => ({ ...current, [key]: event.currentTarget.checked }))} /> {t(`spaceHistory.column.${key}`)}</label>}</For>
            </fieldset>
            <fieldset>
              <legend>{t("spaceHistory.filters")}</legend>
              <label>{t("spaceHistory.actorFilter")}<select class="ui-input" value={actorId()} onChange={(event) => { setActorId(event.currentTarget.value); resetPaging(); }}><option value="">{t("spaceHistory.anyActor")}</option><For each={actorOptions()}>{(id) => <option value={id}>{actorName(id)}</option>}</For></select></label>
              <label>{t("spaceHistory.fromDate")}<input class="ui-input" type="date" value={afterDate()} onInput={(event) => { setAfterDate(event.currentTarget.value); resetPaging(); }} /></label>
              <label>{t("spaceHistory.toDate")}<input class="ui-input" type="date" value={beforeDate()} onInput={(event) => { setBeforeDate(event.currentTarget.value); resetPaging(); }} /></label>
            </fieldset>
            <fieldset>
              <legend>{t("spaceHistory.sort")}</legend>
              <For each={SORT_FIELDS}>{(field) => <label>{t(`spaceHistory.sortField.${field}`)}<select class="ui-input" value={sort().find((item) => item.field === field)?.direction ?? ""} onChange={(event) => { if (event.currentTarget.value) updateSort(field, event.currentTarget.value as "asc" | "desc"); else { setSort((current) => current.filter((item) => item.field !== field)); resetPaging(); } }}><option value="">{t("spaceHistory.unsorted")}</option><option value="asc">{t("entryBrowser.ascending")}</option><option value="desc">{t("entryBrowser.descending")}</option></select></label>}</For>
            </fieldset>
          </div>
        </details>
      </div>
      <Show when={page.loading}>
        <LocalBusyIndicator label={t("spaceHistory.loading")} />
      </Show>
      <Show when={page.error}>
        <p class="ui-text-danger" role="alert">{t("spaceHistory.loadError")}</p>
        <button
          type="button"
          class="ui-button ui-button-secondary"
          disabled={page.loading}
          onClick={() => void refetch()}
        >
          {t("common.retry")}
        </button>
      </Show>
      <Show when={!page.loading && !page.error && currentRows().length === 0}>
        <p class="ui-muted">{t("spaceHistory.empty")}</p>
      </Show>
      <Show when={!page.error && currentRows().length > 0}>
        <HistoryChangeTable
          columns={columns()}
          rows={currentRows()}
          pageIdentity={queryKey()}
          tableLabel={t("spaceHistory.pagination")}
          selectedChangeId={selectedChange()}
          onSelectChange={(row) => setSelectedChange(row.change_id)}
          openLabel={t("spaceHistory.openChange")}
          busy={page.loading}
          onOpenChange={(row, opener) => {
            detailOpener = opener;
            setSelectedChange(row.change_id);
            setDetail(row.change_id);
          }}
        />
      </Show>
      <ResultPagination
        canPrevious={cursors().length > 1}
        canNext={!!page()?.next_cursor}
        busy={page.loading || !!page.error}
        previousLabel={t("common.previous")}
        nextLabel={t("common.next")}
        ariaLabel={t("spaceHistory.pagination")}
        onPrevious={() => {
          setCursors((current) => current.slice(0, -1));
          setSelectedChange(undefined);
        }}
        onNext={() => {
          const next = page()?.next_cursor;
          if (next) {
            setCursors((current) => [...current, next]);
            setSelectedChange(undefined);
          }
        }}
      />
      <Show when={openChange()}>
        {(row) => (
          <Portal>
          <div class="ui-backdrop history-detail-backdrop" onClick={(event) => { if (event.target === event.currentTarget) closeDetail(); }}>
            <section class="ui-dialog history-detail" role="dialog" aria-modal="true" aria-labelledby="history-detail-title" onKeyDown={handleDetailKeyDown}>
              <header class="history-detail-header">
                <div><h2 id="history-detail-title">{detailRow() ? changeTitle(detailRow()!) : t("spaceHistory.detailTitle")}</h2>
                  <p>{inspection() ? formatDateTimeLabel(inspection()!.change.created_at_micros / 1000) : t("spaceHistory.loading")}{inspection() ? ` · ${actorName(inspection()!.change.actor_principal_id)}` : ""}</p></div>
                <IconButton
                  ref={(element) => detailCloseButton = element}
                  icon="close"
                  label={t("common.close")}
                  title={t("common.close")}
                  onClick={() => closeDetail()}
                />
              </header>
              <div class="history-detail-content">
                <Show when={inspection.error}><p class="ui-alert ui-alert-error" role="alert">{t("spaceHistory.loadError")}</p></Show>
                <Show when={inspection.loading}><p>{t("spaceHistory.loading")}</p></Show>
                <Show when={inspection()}>
                  <section><h3>{t("spaceHistory.confirmedBreakdown")}</h3>
                    <Show when={inspection()!.target_visibility === "complete" && inspection()!.summary} fallback={<p>{t("spaceHistory.restrictedSummary")}</p>}>
                      <ul class="history-summary-list"><For each={inspection()!.summary!.field_groups}>{(group) => <li>{fieldName(group.form_id, group.field_id)}: {userValue(group.before)} → {userValue(group.after)} <span>({t("spaceHistory.targetCount", { count: group.affected_entry_count })})</span></li>}</For></ul>
                    </Show>
                  </section>
                  <section class="history-affected-layout"><div><h3>{t("spaceHistory.affectedEntries")}</h3>
                    <ul class="history-affected-list"><For each={targetRows()}>{(target, index) => <li><button type="button" classList={{ selected: selectedEntryId() === target.entry_id }} onClick={() => setSelectedEntryId(target.entry_id)}><span>{formName(target.form_id)} · {t("spaceHistory.entryNumber", { count: index() + 1 })}</span><span aria-hidden="true">›</span></button></li>}</For></ul>
                    <Show when={targetError()}><p role="alert">{t("spaceHistory.loadError")}</p></Show>
                    <Show when={targetLoading()}>
                      <LocalBusyIndicator label={t("spaceHistory.loading")} size="sm" />
                    </Show>
                    <ResultPagination
                      class="history-detail-pagination"
                      canPrevious={targetHistory().length > 1}
                      canNext={!!targetCursor()}
                      busy={targetLoading()}
                      previousLabel={t("common.previous")}
                      nextLabel={t("common.next")}
                      ariaLabel={t("spaceHistory.affectedEntries")}
                      onPrevious={() =>
                        void loadTargetPage(targetHistory().at(-2), "previous")}
                      onNext={() => void loadTargetPage(targetCursor(), "next")}
                    />
                  </div><div class="history-entry-diff"><h3>{t("spaceHistory.selectedEntry")}</h3>
                    <Show when={affectedEntry.loading}><p>{t("spaceHistory.loading")}</p></Show>
                    <Show when={affectedEntry.error}><p role="alert">{t("spaceHistory.loadError")}</p></Show>
                    <Show when={affectedEntry()}><dl><For each={affectedEntry()!.fields}>{(field) => <><dt>{fieldName(affectedEntry()!.form_id, field.field_id)}</dt><dd><span>{userValue(field.before)}</span><span aria-hidden="true"> → </span><span>{userValue(field.after)}</span></dd></>}</For></dl></Show>
                  </div></section>
                  <Show when={inspection()!.change.run_id}><section><h3>{t("spaceHistory.relatedChanges")}</h3><ul class="history-related-list"><For each={relatedChanges().filter((change) => change.change_id !== openChange())}>{(change) => <li><button type="button" onClick={() => setDetail(change.change_id)}>{formatDateTimeLabel(change.change.created_at_micros / 1000)} · {showSummary(change)}</button></li>}</For></ul>
                    <Show when={relatedLoading()}>
                      <LocalBusyIndicator label={t("spaceHistory.loading")} size="sm" />
                    </Show>
                    <ResultPagination
                      class="history-detail-pagination"
                      canPrevious={relatedHistory().length > 1}
                      canNext={!!relatedCursor()}
                      busy={relatedLoading()}
                      previousLabel={t("common.previous")}
                      nextLabel={t("common.next")}
                      ariaLabel={t("spaceHistory.relatedChanges")}
                      onPrevious={() =>
                        void loadRelatedPage(relatedHistory().at(-2), "previous")}
                      onNext={() => void loadRelatedPage(relatedCursor(), "next")}
                    />
                  </section></Show>
                  <details class="history-technical-info"><summary>{t("spaceHistory.technicalInfo")}</summary><dl><dt>Change ID</dt><dd>{inspection()!.change_id}</dd><Show when={inspection()!.change.run_id}><dt>Run ID</dt><dd>{inspection()!.change.run_id}</dd></Show><Show when={selectedEntryId()}><dt>Entry ID</dt><dd>{selectedEntryId()}</dd></Show></dl></details>
                </Show>
              </div>
              <footer class="ui-dialog-actions history-detail-actions">
                <button type="button" class="ui-button ui-button-secondary" disabled={!inspection() || historyNeedsReview() || recoveryBusy()} onClick={() => { setRecoveryNotice(null); setRecoveryFailure(null); setPendingRecovery("revert"); }}>{t("spaceHistory.revertAction")}</button>
                <Show when={inspection()?.change.run_id}><button type="button" class="ui-button ui-button-secondary" disabled={historyNeedsReview() || recoveryBusy()} onClick={() => { setRecoveryNotice(null); setRecoveryFailure(null); setPendingRecovery("undo"); }}>{t("spaceHistory.undoRunAction")}</button></Show>
              </footer>
            </section>
          </div>
          </Portal>
        )}
      </Show>
      <Show when={recoveryNotice()}><p class="ui-alert ui-alert-success" role="status">{recoveryNotice()}</p></Show>
      <Show when={recoveryFailure()}>
        <div class="ui-alert ui-alert-error" role="alert">
          <p>{recoveryFailure()}</p>
          <Show when={historyNeedsReview()}>
            <button type="button" class="ui-button ui-button-secondary mt-2" onClick={() => void reconcileRecovery()}>{t("spaceHistory.retryHistoryRefresh")}</button>
          </Show>
        </div>
      </Show>
      <ConfirmDestructiveAction
        open={pendingRecovery() !== null}
        title={pendingRecovery() === "undo" ? t("spaceHistory.undoRunAction") : t("spaceHistory.revertAction")}
        body={pendingRecovery() === "undo" ? t("spaceHistory.undoConfirmScope") : t("spaceHistory.revertConfirmScope")}
        confirmLabel={pendingRecovery() === "undo" ? t("spaceHistory.confirmUndoRun") : t("spaceHistory.confirmRevert")}
        busy={recoveryBusy()}
        error={null}
        onConfirm={() => void confirmRecovery()}
        onClose={() => { setPendingRecovery(null); setRecoveryMessage(""); }}
      >
        <Show when={pendingRecovery() === "revert"}>
          <label>{t("spaceHistory.messageLabel")}<input class="ui-input" value={recoveryMessage()} onInput={(event) => setRecoveryMessage(event.currentTarget.value)} /></label>
        </Show>
      </ConfirmDestructiveAction>
    </section>
  );
}
