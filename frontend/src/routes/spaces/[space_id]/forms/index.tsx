import { useNavigate, useSearchParams } from "@solidjs/router";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { CreateFormDialog, EditFormDialog } from "~/components/create-dialogs";
import { RowList, RowListButton, RowListItem } from "~/components/RowList";
import { UiIcon } from "~/components/UiIcon";
import { useEntriesRouteContext } from "~/lib/entries-route-context";
import { t } from "~/lib/i18n";
import {
  filterCreatableEntryForms,
  isReservedMetadataForm,
} from "~/lib/metadata-forms";
import { formApi } from "~/lib/ugoite-client";
import type { Form, FormCreatePayload } from "~/lib/types";
import { spaceEntriesPath } from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms" });

export default function SpaceFormsIndexPane() {
  const ctx = useEntriesRouteContext();
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const [query, setQuery] = createSignal("");
  const [showFormDialog, setShowFormDialog] = createSignal(false);
  const [editingForm, setEditingForm] = createSignal<Form | null>(null);
  const [showMetadata, setShowMetadata] = createSignal(false);
  const [redirectedLegacy, setRedirectedLegacy] = createSignal(false);
  const forms = createMemo(() =>
    showMetadata() ? ctx.forms() : filterCreatableEntryForms(ctx.forms())
  );
  const filteredForms = createMemo(() =>
    forms().filter((form) =>
      form.name.toLowerCase().includes(query().trim().toLowerCase())
    )
  );

  // Graceful legacy support: /forms?form=X navigates to the form-scoped
  // Entry list. The Forms page itself stays list-only.
  createEffect(() => {
    const legacy = String(params.form || "");
    if (legacy && !redirectedLegacy()) {
      const spaceId = ctx.spaceId();
      if (!spaceId) return;
      setRedirectedLegacy(true);
      navigate(
        spaceEntriesPath(spaceId, `?form=${encodeURIComponent(legacy)}`),
        { replace: true },
      );
    }
  });

  const createForm = async (payload: FormCreatePayload) => {
    await formApi.create(ctx.spaceId(), payload);
    setShowFormDialog(false);
    await ctx.refetchForms();
    navigate(
      spaceEntriesPath(
        ctx.spaceId(),
        `?form=${encodeURIComponent(payload.name)}`,
      ),
    );
  };
  const updateForm = async (payload: FormCreatePayload) => {
    await formApi.create(ctx.spaceId(), payload);
    setEditingForm(null);
    await ctx.refetchForms();
  };

  return (
    <>
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
        <div class="mx-auto max-w-6xl formsPage">
          <div class="flex flex-wrap items-center justify-between gap-3 entriesHeader">
            <h1 class="ui-page-title">{t("formsPage.heading")}</h1>
            <button
              class="ui-button ui-button-primary text-sm"
              type="button"
              aria-label={t("formsPage.newFormAria")}
              onClick={() => setShowFormDialog(true)}
            >
              {t("formsPage.newButton")}
            </button>
          </div>

          <div class="mt-6">
            <div class="entriesToolbar" role="search">
              <label class="entriesSearch">
                <span class="ui-sr-only">{t("formsPage.find")}</span>
                <span class="entriesSearchIcon" aria-hidden="true">
                  <UiIcon name="search" />
                </span>
                <input
                  type="search"
                  aria-label={t("formsPage.find")}
                  class="ui-input"
                  placeholder={t("formsPage.find")}
                  value={query()}
                  onInput={(event) => setQuery(event.currentTarget.value)}
                />
              </label>
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
            </div>
            <Show when={filteredForms().length === 0}>
              <p class="text-sm ui-muted">{t("formsPage.noForms")}</p>
            </Show>
            <RowList label={t("formsPage.heading")}>
              <For each={filteredForms()}>
                {(form) => (
                  <RowListItem
                    main={
                      <RowListButton
                        onActivate={() =>
                          navigate(
                            spaceEntriesPath(
                              ctx.spaceId(),
                              `?form=${encodeURIComponent(form.name)}`,
                            ),
                          )}
                        primary={
                          <>
                            <span class="glyph" aria-hidden="true">
                              {form.name.slice(0, 1).toUpperCase()}
                            </span>
                            <span class="formRowName">{form.name}</span>
                            <Show when={isReservedMetadataForm(form.name)}>
                              <span
                                class="systemFormIcon"
                                aria-label={t("formsPage.systemForm")}
                                title={t("formsPage.systemForm")}
                              >
                                <UiIcon name="storage" />
                              </span>
                            </Show>
                          </>
                        }
                        chevron
                      />
                    }
                    actions={
                      <button
                        type="button"
                        class="rowListIconButton formRowEdit"
                        aria-label={t("formsPage.editFormAria", {
                          name: form.name,
                        })}
                        title={t("formsPage.editFormAria", {
                          name: form.name,
                        })}
                        onClick={() => setEditingForm(form)}
                      >
                        <UiIcon name="settings" />
                      </button>
                    }
                  />
                )}
              </For>
            </RowList>
          </div>
        </div>
        <CreateFormDialog
          open={showFormDialog()}
          columnTypes={ctx.columnTypes()}
          formNames={ctx.forms().map((form) => form.name)}
          onClose={() => setShowFormDialog(false)}
          onSubmit={createForm}
        />
        <Show when={editingForm()}>
          {(form) => (
            <EditFormDialog
              open={true}
              entryForm={form()}
              columnTypes={ctx.columnTypes()}
              formNames={ctx.forms().map((candidate) => candidate.name)}
              onClose={() => setEditingForm(null)}
              onSubmit={updateForm}
            />
          )}
        </Show>
      </Show>
    </>
  );
}
