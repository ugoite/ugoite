import { useNavigate, useSearchParams } from "@solidjs/router";
import { createEffect, createMemo, createSignal, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import { CreateFormDialog } from "~/components/create-dialogs";
import { EntryBrowser } from "~/components/EntryBrowser";
import {
  createEntryQueryController,
  type EntryProjection,
  type EntryQueryCapabilities,
  type EntryQueryScope,
  systemEntryCapabilities,
} from "~/lib/entry-query";
import { useEntriesRouteContext } from "~/lib/entries-route-context";
import {
  filterCreatableEntryForms,
  isReservedMetadataForm,
} from "~/lib/metadata-forms";
import { formApi } from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";
import type { FormCreatePayload } from "~/lib/types";
import {
  spaceEntriesPath,
  spaceEntryPath,
  spaceFormsPath,
} from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms" });

const fieldProjection = (
  capabilities: EntryQueryCapabilities,
): EntryProjection => {
  const fields = capabilities.fields
    .filter((field) => field.projectable && field.field.kind !== "form")
    .map((field) => field.field);
  return fields.length > 0 ? { kind: "fields", fields } : { kind: "preview" };
};

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
  const formName = createMemo(() =>
    searchParams.form ? String(searchParams.form).trim() : ""
  );
  const selectedForm = createMemo(() =>
    ctx.forms().find((form) => form.name === formName())
  );
  const isReservedForm = createMemo(() =>
    formName() !== "" && isReservedMetadataForm(formName())
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
    formName() ? fieldProjection(capabilities()) : { kind: "preview" },
  );
  let lastQueryConfiguration = "";
  createEffect(() => {
    if (ctx.loadingForms()) return;
    if (formName() && !selectedForm()?.id) return;
    const nextQuery = { scope: queryScope(), filters: [], sort: [] };
    const nextProjection = formName()
      ? fieldProjection(capabilities())
      : { kind: "preview" as const };
    const configuration = JSON.stringify({
      space_id: spaceId(),
      nextQuery,
      nextProjection,
    });
    if (configuration === lastQueryConfiguration) return;
    lastQueryConfiguration = configuration;
    void controller.configure(nextQuery, nextProjection);
  });

  const isUnknownForm = createMemo(() => {
    const name = formName();
    if (!name || ctx.loadingForms()) return false;
    if (isReservedMetadataForm(name)) return false;
    return !selectedForm()?.id;
  });
  const needsFirstFormGuidance = createMemo(() =>
    !formName() && !ctx.loadingForms() &&
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
              {formName() || t("entriesPage.heading")}
            </h1>
            <Show when={formName()}>
              <BackLink
                href={spaceFormsPath(spaceId())}
                label={t("entriesPage.formBack")}
              />
            </Show>
          </div>
        </div>

        <div class="mt-6 entriesBody">
          <Show when={isUnknownForm()}>
            <p class="text-sm ui-muted">
              {t("entriesPage.unknownFormHint", { form: formName() })}
            </p>
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
                ctx.forms().filter((form) => form.id).map((form) => [
                  form.id!,
                  form.name,
                ]),
              )}
              onSelect={(row) => navigate(spaceEntryPath(spaceId(), row.id))}
            />
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
