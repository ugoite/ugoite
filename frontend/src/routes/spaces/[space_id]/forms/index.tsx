import { A, useNavigate, useSearchParams } from "@solidjs/router";
import {
  createEffect,
  createMemo,
  createResource,
  createSignal,
  For,
  onCleanup,
  Show,
} from "solid-js";
import { FormTable } from "~/components/FormTable";
import { CreateFormDialog } from "~/components/create-dialogs";
import { SpaceShell } from "~/components/SpaceShell";
import { formatDateLabel } from "~/lib/date-format";
import { useEntriesRouteContext } from "~/lib/entries-route-context";
import { formApi } from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import { sqlSessionApi } from "~/lib/ugoite-client";
import type { FormCreatePayload } from "~/lib/types";

export default function SpaceFormsIndexPane() {
  const ctx = useEntriesRouteContext();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const [showCreateFormDialog, setShowCreateFormDialog] = createSignal(false);
  const sessionId = createMemo(
    () => (searchParams.session ? String(searchParams.session) : ""),
  );
  const [page, setPage] = createSignal(1);
  const [pageSize] = createSignal(25);
  const selectedFormName = createMemo(
    () => (searchParams.form ? String(searchParams.form) : ""),
  );
  const handleCreateForm = async (payload: FormCreatePayload) => {
    await formApi.create(ctx.spaceId(), payload);
    setShowCreateFormDialog(false);
    ctx.refetchForms();
  };

  const [session, { refetch: refetchSession }] = createResource(
    () => sessionId().trim() || null,
    async (id) => sqlSessionApi.get(ctx.spaceId(), id),
  );

  const [sessionRows] = createResource(
    () => {
      const id = sessionId().trim();
      if (!id || session()?.status !== "ready") return null;
      return { id, offset: (page() - 1) * pageSize(), limit: pageSize() };
    },
    async ({ id, offset, limit }) =>
      sqlSessionApi.rows(ctx.spaceId(), id, offset, limit),
  );

  const selectedFormValue = createMemo(() => selectedFormName().trim());
  const selectableForms = createMemo(() =>
    filterCreatableEntryForms(ctx.forms())
  );

  createEffect(() => {
    if (sessionId().trim()) {
      setPage(1);
      return;
    }
    const selected = selectedFormValue().trim();
    if (selectableForms().some((form) => form.name === selected)) return;
    const first = selectableForms()[0];
    if (first?.name) {
      setSearchParams({ form: first.name }, { replace: true });
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

  const selectedForm = createMemo(() =>
    selectableForms().find((entry) => entry.name === selectedFormValue())
  );

  const sessionEntries = createMemo(() => sessionRows()?.rows || []);
  const sessionFields = createMemo(() => {
    const fields = new Set<string>();
    for (const entry of sessionEntries()) {
      const props = entry.properties || {};
      for (const key of Object.keys(props)) {
        fields.add(key);
      }
    }
    return Array.from(fields);
  });

  const totalCount = createMemo(() =>
    sessionRows()?.totalCount ?? sessionEntries().length
  );
  const totalPages = createMemo(() =>
    Math.max(1, Math.ceil(totalCount() / pageSize()))
  );

  return (
    <SpaceShell
      spaceId={ctx.spaceId()}
      activeBottomTab="grid"
    >
      <div class="mx-auto max-w-7xl">
        <div class="ui-forms-workspace">
          <aside class="ui-form-list" aria-label="Forms">
            <div class="ui-form-list-heading">
              <span>Forms</span>
              <button
                type="button"
                class="ui-button ui-button-primary text-xs"
                onClick={() => setShowCreateFormDialog(true)}
              >
                New
              </button>
            </div>
            <select class="ui-sr-only" aria-label={t("formsPage.selectPlaceholder")}>
              <For each={selectableForms()}>
                {(form) => <option value={form.name}>{form.name}</option>}
              </For>
            </select>
            <For each={selectableForms()}>
              {(form) => (
                <a
                  href={`/spaces/${ctx.spaceId()}/forms?form=${encodeURIComponent(form.name)}`}
                  class="ui-form-list-item"
                  classList={{ "ui-form-list-item-active": selectedFormValue() === form.name }}
                >
                  {form.name}
                </a>
              )}
            </For>
            <Show when={!ctx.loadingForms() && selectableForms().length === 0}>
              <p class="px-2 py-3 text-sm ui-muted">Create a form to start adding entries.</p>
            </Show>
          </aside>

          <section>
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h1 class="ui-page-title">
              {sessionId().trim()
                ? t("querySession.heading")
                : t("formsPage.heading")}
            </h1>
            <p class="text-sm ui-muted">
              {sessionId().trim()
                ? t("querySession.formsDescription")
                : t("formsPage.description")}
            </p>
          </div>
          <div class="flex items-center gap-2">
            <Show when={!sessionId().trim()}>
              <button
                type="button"
                class="ui-button ui-button-primary text-sm"
                onClick={() => setShowCreateFormDialog(true)}
              >
                {t("formsPage.newButton")}
              </button>
            </Show>
            <Show when={sessionId().trim()}>
              <button
                type="button"
                class="ui-button ui-button-secondary text-sm"
                onClick={() => navigate(`/spaces/${ctx.spaceId()}/forms`)}
              >
                {t("querySession.clear")}
              </button>
            </Show>
          </div>
        </div>

        <div class="mt-6">
          <Show when={sessionId().trim()}>
            <div class="ui-stack-sm">
              <Show when={session()?.status === "running"}>
                <p class="text-sm ui-muted">{t("querySession.preparing")}</p>
              </Show>
              <Show when={session()?.status === "failed"}>
                <p class="text-sm ui-text-danger">
                  {session()?.error || t("querySession.failed")}
                </p>
              </Show>
              <Show when={session()?.status === "expired"}>
                <p class="text-sm ui-text-danger">
                  {t("querySession.expired")}
                </p>
              </Show>
              <Show when={sessionRows.loading}>
                <p class="text-sm ui-muted">
                  {t("querySession.loadingResults")}
                </p>
              </Show>
              <Show
                when={!sessionRows.loading && sessionEntries().length === 0}
              >
                <p class="text-sm ui-muted">{t("querySession.noResults")}</p>
              </Show>
              <Show when={sessionEntries().length > 0}>
                <div class="ui-table-wrapper overflow-x-auto">
                  <table class="ui-table text-sm min-w-full">
                    <thead class="ui-table-head">
                      <tr>
                        <th class="ui-table-header-cell">
                          {t("common.title")}
                        </th>
                        <th class="ui-table-header-cell">{t("common.form")}</th>
                        <th class="ui-table-header-cell">
                          {t("common.updated")}
                        </th>
                        <For each={sessionFields()}>
                          {(field) => (
                            <th class="ui-table-header-cell">{field}</th>
                          )}
                        </For>
                      </tr>
                    </thead>
                    <tbody class="ui-table-body">
                      <For each={sessionEntries()}>
                        {(entry) => (
                          <tr class="ui-table-row">
                            <td class="ui-table-cell">
                              <button
                                type="button"
                                class="text-left hover:underline"
                                onClick={() =>
                                  navigate(
                                    `/spaces/${ctx.spaceId()}/entries/${
                                      encodeURIComponent(entry.id)
                                    }`,
                                  )}
                              >
                                {entry.title || t("common.untitled")}
                              </button>
                            </td>
                            <td class="ui-table-cell ui-table-cell-muted">
                              {entry.form || "-"}
                            </td>
                            <td class="ui-table-cell ui-table-cell-muted">
                              {formatDateLabel(entry.updated_at)}
                            </td>
                            <For each={sessionFields()}>
                              {(field) => (
                                <td class="ui-table-cell ui-table-cell-muted">
                                  {String(entry.properties?.[field] ?? "")}
                                </td>
                              )}
                            </For>
                          </tr>
                        )}
                      </For>
                    </tbody>
                  </table>
                </div>
              </Show>
              <Show when={totalCount() > 0}>
                <div class="flex flex-wrap items-center justify-between gap-3 text-sm ui-muted">
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
          </Show>
          <Show when={!sessionId().trim()}>
            <Show
              when={selectedForm()}
              fallback={<p class="text-sm ui-muted">{t("formsPage.empty")}</p>}
            >
              {(form) => (
                <>
                  <div class="ui-form-context mb-4">
                    <div>
                      <small>Forms / {form().name} / Entries</small>
                      <h2 class="text-xl font-semibold">{form().name}</h2>
                    </div>
                    <div class="flex gap-1 text-sm">
                      <span class="ui-pill">Entries</span>
                      <a class="ui-button ui-button-secondary text-xs" href={`/spaces/${ctx.spaceId()}/forms/${encodeURIComponent(form().name)}`}>Fields</a>
                      <a class="ui-button ui-button-secondary text-xs" href={`/spaces/${ctx.spaceId()}/assets`}>Assets</a>
                      <a class="ui-button ui-button-secondary text-xs" href={`/spaces/${ctx.spaceId()}/query`}>Views</a>
                    </div>
                  </div>
                  <FormTable
                    spaceId={ctx.spaceId()}
                    entryForm={form()}
                    onEntryClick={(entryId) =>
                      navigate(
                        `/spaces/${ctx.spaceId()}/entries/${
                          encodeURIComponent(entryId)
                        }`,
                      )}
                  />
                </>
              )}
            </Show>
          </Show>
        </div>
          </section>
        </div>
      </div>

      <CreateFormDialog
        open={showCreateFormDialog()}
        columnTypes={ctx.columnTypes()}
        formNames={ctx.forms().map((form) => form.name)}
        onClose={() => setShowCreateFormDialog(false)}
        onSubmit={handleCreateForm}
      />
    </SpaceShell>
  );
}
