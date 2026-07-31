import { useNavigate, useSearchParams } from "@solidjs/router";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { CreateFormDialog, EditFormDialog } from "~/components/create-dialogs";
import { FormTable } from "~/components/FormTable";
import { SpaceShell } from "~/components/SpaceShell";
import { UiIcon } from "~/components/UiIcon";
import { useEntriesRouteContext } from "~/lib/entries-route-context";
import { t } from "~/lib/i18n";
import {
  filterCreatableEntryForms,
  isReservedMetadataForm,
} from "~/lib/metadata-forms";
import { formApi } from "~/lib/ugoite-client";
import type { FormCreatePayload } from "~/lib/types";

export default function SpaceFormsIndexPane() {
  const ctx = useEntriesRouteContext();
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const [query, setQuery] = createSignal("");
  const [showFormDialog, setShowFormDialog] = createSignal(false);
  const [showEditDialog, setShowEditDialog] = createSignal(false);
  const [showMetadata, setShowMetadata] = createSignal(false);
  const forms = createMemo(() =>
    showMetadata() ? ctx.forms() : filterCreatableEntryForms(ctx.forms())
  );
  const selectedName = createMemo(() => String(params.form || ""));
  const selectedForm = createMemo(() =>
    forms().find((form) => form.name === selectedName())
  );
  const filteredForms = createMemo(() =>
    forms().filter((form) =>
      form.name.toLowerCase().includes(query().trim().toLowerCase())
    )
  );
  createEffect(() => {
    if (!selectedForm() && forms()[0]) {
      setParams({ form: forms()[0].name, tab: undefined }, { replace: true });
    }
  });

  const createForm = async (payload: FormCreatePayload) => {
    await formApi.create(ctx.spaceId(), payload);
    setShowFormDialog(false);
    await ctx.refetchForms();
    setParams({ form: payload.name, tab: undefined });
  };
  const updateForm = async (payload: FormCreatePayload) => {
    await formApi.create(ctx.spaceId(), payload);
    setShowEditDialog(false);
    await ctx.refetchForms();
  };

  return (
    <SpaceShell
      spaceId={ctx.spaceId()}
      activeNavigation="forms"
      title={t("spaceShell.bottom.grid")}
    >
      <Show
        when={!ctx.formsError?.()}
        fallback={
          <div class="settingsMain surface ui-stack-sm">
            <p class="ui-alert ui-alert-error">{t("formsPage.failedLoad")}</p>
            <button
              class="btn"
              type="button"
              onClick={() => ctx.refetchForms()}
            >
              {t("formsPage.retry")}
            </button>
          </div>
        }
      >
        <div class="split">
          <aside class="listPane surface">
            <div class="paneHead">
              <b>{t("spaceShell.bottom.grid")}</b>
              <button
                class="btn iconBtn"
                type="button"
                aria-label={t("formsPage.newFormAria")}
                onClick={() => setShowFormDialog(true)}
              >
                <UiIcon name="plus" />
              </button>
            </div>
            <label class="formVisibilityToggle">
              <span>{t("formsPage.showMetadata")}</span>
              <input
                type="checkbox"
                checked={showMetadata()}
                onChange={(event) =>
                  setShowMetadata(event.currentTarget.checked)}
              />
              <span class="formVisibilityTrack" aria-hidden="true" />
            </label>
            <label class="miniSearch">
              <UiIcon name="search" />
              <input
                value={query()}
                onInput={(event) => setQuery(event.currentTarget.value)}
                placeholder={t("formsPage.find")}
              />
            </label>
            <For
              each={filteredForms()}
              fallback={
                <div class="ui-muted p-3">{t("formsPage.noForms")}</div>
              }
            >
              {(form) => (
                <button
                  class="formItem"
                  classList={{ active: selectedName() === form.name }}
                  type="button"
                  onClick={() => setParams({ form: form.name, tab: undefined })}
                >
                  <span
                    class="glyph"
                    classList={{ active: selectedName() === form.name }}
                  >
                    {form.name.slice(0, 1).toUpperCase()}
                  </span>
                  <span>
                    <b>{form.name}</b>
                    <Show when={isReservedMetadataForm(form.name)}>
                      <span
                        class="systemFormIcon"
                        aria-label={t("formsPage.systemForm")}
                        title={t("formsPage.systemForm")}
                      >
                        <UiIcon name="storage" />
                      </span>
                    </Show>
                  </span>
                  <span class="chev">›</span>
                </button>
              )}
            </For>
          </aside>
          <main class="detailPane">
            <Show
              when={selectedForm()}
              fallback={
                <div class="surface settingsMain ui-muted">
                  {t("formsPage.selectPlaceholder")}
                </div>
              }
            >
              {(form) => (
                <>
                  <div class="formWorkspaceHead surface">
                    <div class="actions">
                      <button
                        class="btn"
                        type="button"
                        onClick={() => setShowEditDialog(true)}
                      >
                        <UiIcon name="settings" /> {t("formsPage.editForm")}
                      </button>
                      <button
                        class="btn primary"
                        type="button"
                        onClick={() =>
                          navigate(
                            `/spaces/${ctx.spaceId()}/entries/new?form=${
                              encodeURIComponent(form().name)
                            }`,
                          )}
                      >
                        <UiIcon name="plus" /> {t("formsPage.newEntry")}
                      </button>
                    </div>
                  </div>
                  <FormTable
                    spaceId={ctx.spaceId()}
                    entryForm={form()}
                    onEntryClick={(id) =>
                      navigate(
                        `/spaces/${ctx.spaceId()}/entries/${
                          encodeURIComponent(id)
                        }`,
                      )}
                  />
                  <EditFormDialog
                    open={showEditDialog()}
                    entryForm={form()}
                    columnTypes={ctx.columnTypes()}
                    formNames={ctx.forms().map((candidate) => candidate.name)}
                    onClose={() => setShowEditDialog(false)}
                    onSubmit={updateForm}
                  />
                </>
              )}
            </Show>
          </main>
        </div>
        <CreateFormDialog
          open={showFormDialog()}
          columnTypes={ctx.columnTypes()}
          formNames={ctx.forms().map((form) => form.name)}
          onClose={() => setShowFormDialog(false)}
          onSubmit={createForm}
        />
      </Show>
    </SpaceShell>
  );
}
