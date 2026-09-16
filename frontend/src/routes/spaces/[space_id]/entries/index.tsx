import { A, useNavigate, useSearchParams } from "@solidjs/router";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  Show,
} from "solid-js";
import { CreateFormDialog } from "~/components/create-dialogs";
import { formatDateLabel } from "~/lib/date-format";
import { useEntriesRouteContext } from "~/lib/entries-route-context";
import { formApi, searchApi } from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";
import { createResource } from "~/lib/recoverable-resource";
import {
  filterCreatableEntryForms,
  isReservedMetadataForm,
} from "~/lib/metadata-forms";
import { sqlSessionApi, sqlSessionRowToEntryRecord } from "~/lib/ugoite-client";
import type { EntryRecord, FormCreatePayload } from "~/lib/types";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms" });

export default function SpaceEntriesIndexPane() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const ctx = useEntriesRouteContext();
  const spaceId = () => ctx.spaceId();
  const [showCreateFormDialog, setShowCreateFormDialog] = createSignal(false);
  const creatableForms = createMemo(() =>
    filterCreatableEntryForms(ctx.forms())
  );
  const hasCreatableForms = createMemo(() => creatableForms().length > 0);

  const sessionId = createMemo(
    () => (searchParams.session ? String(searchParams.session) : ""),
  );
  const formName = createMemo(
    () => (searchParams.form ? String(searchParams.form).trim() : ""),
  );
  const isReservedForm = createMemo(
    () => formName() !== "" && isReservedMetadataForm(formName()),
  );
  const [page, setPage] = createSignal(1);
  const [pageSize] = createSignal(24);

  const [session, { refetch: refetchSession }] = createResource(
    () => sessionId().trim() || null,
    async (id) => sqlSessionApi.get(spaceId(), id),
  );

  const [sessionRows] = createResource(
    () => {
      const id = sessionId().trim();
      if (!id || session()?.status !== "ready") return null;
      return { id, offset: (page() - 1) * pageSize(), limit: pageSize() };
    },
    async ({ id, offset, limit }) =>
      sqlSessionApi.rows(spaceId(), id, offset, limit),
  );

  // Form-scoped Entry list: server-side query, never a client-side filter
  // over the unpaginated entry store.
  const [formEntries] = createResource(
    () => {
      if (sessionId().trim() || !formName()) return null;
      return { id: spaceId(), form: formName() };
    },
    async ({ id, form }) => await searchApi.query(id, { form }),
  );

  createEffect(() => {
    if (spaceId() && !sessionId().trim() && !formName()) {
      ctx.entryStore.loadEntries();
    }
  });

  createEffect(() => {
    const id = sessionId().trim();
    if (!id) return;
    const interval = setInterval(() => {
      if (session()?.status === "running") {
        refetchSession();
      }
    }, 1000);
    onCleanup(() => clearInterval(interval));
  });

  createEffect(() => {
    if (sessionId().trim()) {
      setPage(1);
    }
  });

  const displayEntryState = createMemo<{
    entries: EntryRecord[];
    error: Error | null;
  }>(() => {
    if (!sessionId().trim() && formName()) {
      return { entries: formEntries() ?? [], error: null };
    }
    if (!sessionId().trim()) {
      return { entries: ctx.entryStore.entries() || [], error: null };
    }
    const rows = sessionRows()?.rows;
    if (!rows) return { entries: [], error: null };
    try {
      return { entries: rows.map(sqlSessionRowToEntryRecord), error: null };
    } catch (error) {
      return {
        entries: [],
        error: error instanceof Error ? error : new Error(String(error)),
      };
    }
  });

  const displayEntries = createMemo(() => displayEntryState().entries);

  type EntrySort = "updated" | "title";
  const [entryQuery, setEntryQuery] = createSignal("");
  const [entrySort, setEntrySort] = createSignal<EntrySort>("updated");
  const visibleEntries = createMemo(() => {
    const query = entryQuery().trim().toLocaleLowerCase();
    const filtered = query
      ? displayEntries().filter((entry) =>
        [entry.title, entry.form].some((value) =>
          value?.toLocaleLowerCase().includes(query)
        )
      )
      : displayEntries();

    return [...filtered].sort((left, right) => {
      if (entrySort() === "title") {
        return (left.title || t("common.untitled")).localeCompare(
          right.title || t("common.untitled"),
        );
      }

      const leftUpdated = Date.parse(left.updated_at);
      const rightUpdated = Date.parse(right.updated_at);
      const leftTime = Number.isNaN(leftUpdated)
        ? Number.NEGATIVE_INFINITY
        : leftUpdated;
      const rightTime = Number.isNaN(rightUpdated)
        ? Number.NEGATIVE_INFINITY
        : rightUpdated;
      return rightTime - leftTime || left.id.localeCompare(right.id);
    });
  });

  const totalCount = createMemo(() =>
    sessionRows()?.totalCount ?? displayEntries().length
  );

  const totalPages = createMemo(() =>
    Math.max(1, Math.ceil(totalCount() / pageSize()))
  );

  const isLoading = createMemo(() => {
    if (sessionId().trim()) {
      return session.loading || sessionRows.loading;
    }
    if (formName()) {
      return formEntries.loading;
    }
    return ctx.entryStore.loading();
  });

  const error = createMemo<unknown>(() => {
    if (sessionId().trim()) {
      return session.error || sessionRows.error || displayEntryState().error;
    }
    if (formName()) {
      return formEntries.error || displayEntryState().error;
    }
    return ctx.entryStore.errorCause();
  });

  const errorMessage = createMemo(() => {
    const err = error();
    if (!err) return null;
    if (formName() && !sessionId().trim()) {
      return formatUserFacingError(err, "formTable.recordsError", "search.query");
    }
    return formatUserFacingError(
      err,
      "formTable.recordsError",
      sessionId().trim() ? "sql_session.rows" : undefined,
    );
  });
  const needsFirstFormGuidance = createMemo(
    () =>
      !sessionId().trim() &&
      !formName() &&
      !isLoading() &&
      !ctx.loadingForms() &&
      displayEntries().length === 0 &&
      !errorMessage() &&
      !hasCreatableForms(),
  );

  const handleSelectEntry = (entryId: string) => {
    navigate(`/spaces/${spaceId()}/entries/${encodeURIComponent(entryId)}`);
  };

  const handleCreateForm = async (payload: FormCreatePayload) => {
    await formApi.create(spaceId(), payload);
    setShowCreateFormDialog(false);
    void ctx.refetchForms();
  };

  return (
    <>
      <div class="mx-auto max-w-6xl entriesPage">
        <div class="flex flex-wrap items-center justify-between gap-3 entriesHeader">
          <div>
            <h1 class="ui-page-title">
              {sessionId().trim()
                ? t("querySession.heading")
                : formName() || t("entriesPage.heading")}
            </h1>
            <Show when={sessionId().trim()}>
              <p class="text-sm ui-muted">
                {t("entriesPage.queryDescription")}
              </p>
            </Show>
            <Show when={!sessionId().trim() && formName()}>
              <A
                class="text-sm ui-focus-text"
                href={`/spaces/${spaceId()}/forms`}
              >
                {t("entriesPage.formBack")}
              </A>
            </Show>
          </div>
          <div class="flex items-center gap-2">
            <Show when={sessionId().trim()}>
              <button
                type="button"
                class="ui-button ui-button-secondary text-sm"
                onClick={() => navigate(`/spaces/${spaceId()}/forms`)}
              >
                {t("querySession.clear")}
              </button>
            </Show>
            <Show when={!formName() || !isReservedForm()}>
              <button
                type="button"
                class="ui-button text-sm"
                classList={{
                  "ui-button-primary": hasCreatableForms(),
                  "ui-button-secondary": !hasCreatableForms(),
                }}
                disabled={!hasCreatableForms()}
                onClick={() =>
                  navigate(
                    formName()
                      ? `/spaces/${spaceId()}/entries/new?form=${
                        encodeURIComponent(formName())
                      }`
                      : `/spaces/${spaceId()}/entries/new`,
                  )}
              >
                {t("entriesPage.newButton")}
              </button>
            </Show>
          </div>
        </div>

        <div class="mt-6 entriesBody">
          <Show when={sessionId().trim() && session()?.status === "running"}>
            <p class="text-sm ui-muted">{t("querySession.preparing")}</p>
          </Show>
          <Show when={session()?.status === "failed"}>
            <p class="text-sm ui-text-danger">
              {session()?.error || t("querySession.failed")}
            </p>
          </Show>
          <Show when={session()?.status === "expired"}>
            <p class="text-sm ui-text-danger">{t("querySession.expired")}</p>
          </Show>
          <Show when={isLoading()}>
            <p class="text-sm ui-muted">{t("listPanel.loadingEntries")}</p>
          </Show>
          <Show when={errorMessage()}>
            <p class="text-sm ui-text-danger">{errorMessage()}</p>
          </Show>
          <Show
            when={!needsFirstFormGuidance() &&
              !isLoading() &&
              displayEntries().length === 0 &&
              !errorMessage()}
          >
            <p class="text-sm ui-muted">{t("entriesPage.noEntries")}</p>
          </Show>
          <Show
            when={!sessionId().trim() && !isLoading() && !errorMessage()}
          >
            <div class="entriesToolbar" role="search">
              <label class="entriesSearch">
                <span class="ui-sr-only">{t("entriesPage.filterLabel")}</span>
                <span class="entriesSearchIcon" aria-hidden="true">⌕</span>
                <input
                  type="search"
                  aria-label={t("entriesPage.filterLabel")}
                  class="ui-input"
                  placeholder={t("entriesPage.filterPlaceholder")}
                  value={entryQuery()}
                  onInput={(event) => setEntryQuery(event.currentTarget.value)}
                />
              </label>
              <label class="entriesSort">
                <span class="ui-sr-only">{t("entriesPage.sortLabel")}</span>
                <select
                  class="ui-select"
                  aria-label={t("entriesPage.sortLabel")}
                  value={entrySort()}
                  onChange={(event) =>
                    setEntrySort(event.currentTarget.value as EntrySort)}
                >
                  <option value="updated">
                    {t("entriesPage.sortUpdated")}
                  </option>
                  <option value="title">{t("entriesPage.sortTitle")}</option>
                </select>
              </label>
              <span class="entriesCount ui-muted">
                {t("entriesPage.count", { count: visibleEntries().length })}
              </span>
            </div>
          </Show>
          <Show
            when={!isLoading() && !errorMessage() &&
              displayEntries().length > 0 && visibleEntries().length === 0}
          >
            <p class="text-sm ui-muted">{t("entriesPage.noMatches")}</p>
          </Show>
          <Show when={needsFirstFormGuidance()}>
            <div class="ui-alert ui-alert-warning mb-4 text-sm ui-stack-sm">
              <div class="ui-stack-sm">
                <p class="font-medium">
                  {t("dashboard.section.createEntry.empty")}
                </p>
                <p>{t("dashboard.section.createEntry.firstFormDescription")}</p>
              </div>
              <div>
                <button
                  type="button"
                  class="ui-button ui-button-primary text-sm"
                  onClick={() => setShowCreateFormDialog(true)}
                >
                  {t("dashboard.section.createEntry.createFirstForm")}
                </button>
              </div>
            </div>
          </Show>
          <div class="entriesList">
            <For each={visibleEntries()}>
              {(entry) => (
                <button
                  type="button"
                  class="entryRow"
                  onClick={() => handleSelectEntry(entry.id)}
                >
                  <span class="entryRowMain">
                    <span class="entryRowTitle">
                      {entry.title || t("common.untitled")}
                    </span>
                    <Show when={entry.form}>
                      <span class="ui-pill entryRowForm">{entry.form}</span>
                    </Show>
                  </span>
                  <span class="entryRowDate ui-muted">
                    {t("common.updatedAt", {
                      date: formatDateLabel(entry.updated_at),
                    })}
                  </span>
                  <span class="entryRowChevron" aria-hidden="true">›</span>
                </button>
              )}
            </For>
          </div>
          <Show
            when={!sessionId().trim() && !formName() && ctx.entryStore.hasMore()}
          >
            <div class="mt-6 flex justify-center">
              <button
                type="button"
                class="ui-button ui-button-secondary text-sm"
                disabled={ctx.entryStore.loadingMore()}
                onClick={() => void ctx.entryStore.loadMoreEntries()}
              >
                {ctx.entryStore.loadingMore()
                  ? t("entriesPage.loadingMore")
                  : t("entriesPage.loadMore")}
              </button>
            </div>
          </Show>
          <Show when={sessionId().trim() && totalCount() > 0}>
            <div class="mt-6 flex flex-wrap items-center justify-between gap-3 text-sm ui-muted">
              <div>
                {t("querySession.pagination", {
                  page: page(),
                  totalPages: totalPages(),
                  resultCount: totalCount(),
                })}
              </div>
              <div class="flex items-center gap-2">
                <button
                  type="button"
                  class="ui-button ui-button-secondary text-sm"
                  disabled={page() <= 1}
                  onClick={() => setPage((prev) => Math.max(1, prev - 1))}
                >
                  {t("common.previous")}
                </button>
                <button
                  type="button"
                  class="ui-button ui-button-secondary text-sm"
                  disabled={page() >= totalPages()}
                  onClick={() =>
                    setPage((prev) => Math.min(totalPages(), prev + 1))}
                >
                  {t("common.next")}
                </button>
              </div>
            </div>
          </Show>
        </div>
      </div>

      <CreateFormDialog
        open={showCreateFormDialog()}
        columnTypes={ctx.columnTypes()}
        formNames={ctx.forms().map((form) => form.name)}
        onClose={() => setShowCreateFormDialog(false)}
        onSubmit={handleCreateForm}
      />
    </>
  );
}
