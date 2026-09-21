import { useNavigate, useSearchParams } from "@solidjs/router";
import { createEffect, createMemo, createSignal, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import { CreateFormDialog } from "~/components/create-dialogs";
import { EntryBrowser } from "~/components/EntryBrowser";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import {
  createEntryQueryController,
  type EntryQueryCapabilities,
  type EntryQueryScope,
  systemEntryCapabilities,
} from "~/lib/entry-query";
import { useEntriesRouteContext } from "~/lib/entries-route-context";
import {
  filterCreatableEntryForms,
  isReservedMetadataForm,
} from "~/lib/metadata-forms";
import {
  formApi,
  sqlSessionApi,
  sqlSessionRowToEntryRecord,
} from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";
import { createResource } from "~/lib/recoverable-resource";
import type { EntryRecord, FormCreatePayload } from "~/lib/types";
import { formatUserFacingError } from "~/lib/user-facing-error";
import {
  spaceEntriesPath,
  spaceEntryPath,
  spaceFormsPath,
} from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "entries" });

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
  const sessionId = createMemo(() =>
    searchParams.session ? String(searchParams.session) : ""
  );
  const formName = createMemo(() =>
    searchParams.form ? String(searchParams.form).trim() : ""
  );
  const selectedForm = createMemo(() =>
    ctx.forms().find((form) => form.name === formName())
  );
  const isReservedForm = createMemo(() =>
    formName() !== "" && isReservedMetadataForm(formName())
  );

  const [session, { refetch: refetchSession }] = createResource(
    () => sessionId().trim() || null,
    async (id) => sqlSessionApi.get(spaceId(), id),
  );
  const [page, setPage] = createSignal(1);
  const [pageSize] = createSignal(24);
  const [sessionRows] = createResource(
    () => {
      const id = sessionId().trim();
      if (!id || session()?.status !== "ready") return null;
      return { id, offset: (page() - 1) * pageSize(), limit: pageSize() };
    },
    async ({ id, offset, limit }) =>
      sqlSessionApi.rows(spaceId(), id, offset, limit),
  );

  const queryScope = createMemo<EntryQueryScope>(() =>
    selectedForm()?.id
      ? { kind: "form", form_id: selectedForm()!.id! }
      : { kind: "all" }
  );
  const capabilities = createMemo<EntryQueryCapabilities>(() => {
    const scope = queryScope();
    const system = systemEntryCapabilities(scope);
    const formCapabilities = selectedForm()
      ? Object.values(selectedForm()!.fields)
        .map((field) => field.query_capability)
        .filter((field): field is NonNullable<typeof field> =>
          field !== undefined
        )
      : [];
    return { scope, fields: [...system.fields, ...formCapabilities] };
  });

  const controller = createEntryQueryController(
    () => spaceId(),
    { scope: queryScope(), filters: [], sort: [] },
    { kind: "preview" },
  );
  let lastQueryConfiguration = "";
  createEffect(() => {
    if (sessionId().trim() || ctx.loadingForms()) return;
    if (formName() && !selectedForm()?.id) return;
    const nextQuery = { scope: queryScope(), filters: [], sort: [] };
    const nextProjection = { kind: "preview" as const };
    const configuration = JSON.stringify({
      space_id: spaceId(),
      nextQuery,
      nextProjection,
    });
    if (configuration === lastQueryConfiguration) return;
    lastQueryConfiguration = configuration;
    void controller.configure(nextQuery, nextProjection);
  });

  createEffect(() => {
    const id = sessionId().trim();
    if (!id) return;
    const interval = setInterval(() => {
      if (session()?.status === "running") refetchSession();
    }, 1000);
    return () => clearInterval(interval);
  });

  createEffect(() => {
    if (sessionId().trim()) setPage(1);
  });

  const isUnknownForm = createMemo(() => {
    const name = formName();
    if (!name || sessionId().trim() || ctx.loadingForms()) return false;
    if (isReservedMetadataForm(name)) return false;
    return !selectedForm()?.id;
  });
  const sessionEntries = createMemo<
    { entries: EntryRecord[]; error: Error | null }
  >(() => {
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
  const sessionTotal = createMemo(() => sessionRows()?.totalCount ?? 0);
  const sessionTotalPages = createMemo(() =>
    Math.max(1, Math.ceil(sessionTotal() / pageSize()))
  );
  const isLoading = createMemo(() =>
    sessionId().trim() ? session.loading || sessionRows.loading : false
  );
  const errorMessage = createMemo(() => {
    const error = sessionId().trim()
      ? session.error || sessionRows.error || sessionEntries().error
      : null;
    return error
      ? formatUserFacingError(
        error,
        "formTable.recordsError",
        sessionId().trim() ? "sql_session.rows" : "entry.query",
      )
      : null;
  });
  const needsFirstFormGuidance = createMemo(() =>
    !sessionId().trim() && !formName() && !ctx.loadingForms() &&
    !hasCreatableForms()
  );

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
              <BackLink
                href={spaceFormsPath(spaceId())}
                label={t("entriesPage.formBack")}
              />
            </Show>
          </div>
          <Show when={sessionId().trim()}>
            <button
              type="button"
              class="ui-button ui-button-secondary text-sm"
              onClick={() => navigate(spaceFormsPath(spaceId()))}
            >
              {t("querySession.clear")}
            </button>
          </Show>
        </div>

        <div class="mt-6 entriesBody" aria-busy={isLoading() || undefined}>
          <Show
            when={sessionId().trim()}
            fallback={
              <>
                <Show when={isUnknownForm()}>
                  <p class="text-sm ui-muted">
                    {t("entriesPage.unknownFormHint", { form: formName() })}
                  </p>
                </Show>
                <Show when={errorMessage()}>
                  <p class="text-sm ui-text-danger">{errorMessage()}</p>
                </Show>
                <Show when={needsFirstFormGuidance()}>
                  <div class="ui-alert ui-alert-warning mb-4 text-sm ui-stack-sm">
                    <p class="font-medium">
                      {t("dashboard.section.createEntry.empty")}
                    </p>
                    <p>
                      {t("dashboard.section.createEntry.firstFormDescription")}
                    </p>
                    <button
                      type="button"
                      class="ui-button ui-button-primary text-sm"
                      onClick={() => setShowCreateFormDialog(true)}
                    >
                      {t("dashboard.section.createEntry.createFirstForm")}
                    </button>
                  </div>
                </Show>
                <Show when={!isReservedForm()}>
                  <div class="entriesCreateRow">
                    <button
                      type="button"
                      class="ui-button ui-button-primary text-sm"
                      disabled={!hasCreatableForms()}
                      onClick={() =>
                        navigate(
                          formName()
                            ? spaceEntriesPath(
                              spaceId(),
                              `/new?form=${encodeURIComponent(formName())}`,
                            )
                            : spaceEntriesPath(spaceId(), "/new"),
                        )}
                    >
                      {t("entriesPage.newShort")}
                    </button>
                  </div>
                </Show>
                <Show when={!isReservedForm() && !isUnknownForm()}>
                  <EntryBrowser
                    controller={controller}
                    capabilities={capabilities()}
                    formLabels={Object.fromEntries(
                      ctx.forms().filter((form) => form.id).map((
                        form,
                      ) => [form.id!, form.name]),
                    )}
                    onSelect={(row) =>
                      navigate(spaceEntryPath(spaceId(), row.id))}
                  />
                </Show>
              </>
            }
          >
            <Show when={session()?.status === "running"}>
              <LocalBusyIndicator label={t("querySession.preparing")} />
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
              <LocalBusyIndicator label={t("listPanel.loadingEntries")} />
            </Show>
            <Show when={errorMessage()}>
              <p class="text-sm ui-text-danger">{errorMessage()}</p>
            </Show>
            <Show
              when={!isLoading() && !errorMessage() &&
                sessionEntries().entries.length === 0}
            >
              <p class="text-sm ui-muted">{t("entriesPage.noEntries")}</p>
            </Show>
            <div class="entriesList">
              {sessionEntries().entries.map((entry) => (
                <button
                  type="button"
                  class="entryRow"
                  onClick={() => navigate(spaceEntryPath(spaceId(), entry.id))}
                >
                  <span class="entryRowTitle">{entry.id}</span>
                  <span class="entryRowDate ui-muted">
                    {formatDateLabel(entry.updated_at)}
                  </span>
                </button>
              ))}
            </div>
            <Show when={sessionTotal() > 0}>
              <div class="mt-6 flex flex-wrap items-center justify-between gap-3 text-sm ui-muted">
                <span>
                  {t("querySession.pagination", {
                    page: page(),
                    totalPages: sessionTotalPages(),
                    resultCount: sessionTotal(),
                  })}
                </span>
                <span class="flex gap-2">
                  <button
                    type="button"
                    class="ui-button ui-button-secondary"
                    disabled={page() <= 1}
                    onClick={() => setPage((value) => Math.max(1, value - 1))}
                  >
                    {t("common.previous")}
                  </button>
                  <button
                    type="button"
                    class="ui-button ui-button-secondary"
                    disabled={page() >= sessionTotalPages()}
                    onClick={() =>
                      setPage((value) =>
                        Math.min(sessionTotalPages(), value + 1)
                      )}
                  >
                    {t("common.next")}
                  </button>
                </span>
              </div>
            </Show>
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
