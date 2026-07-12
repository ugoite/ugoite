import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, Show } from "solid-js";
import {
  CreateEntryDialog,
  CreateFormDialog,
} from "~/components/create-dialogs";
import { SpaceShell } from "~/components/SpaceShell";
import { createEntryStore } from "~/lib/entry-store";
import {
  buildEntryMarkdownByMode,
  type EntryInputMode,
} from "~/lib/entry-input";
import { t } from "~/lib/i18n";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import { formApi } from "~/lib/ugoite-client";
import { spaceApi } from "~/lib/ugoite-client";
import type { FormCreatePayload } from "~/lib/types";

export default function SpaceDashboardRoute() {
  const params = useParams<{ space_id: string }>();
  const spaceId = () => params.space_id;
  const navigate = useNavigate();
  const entryStore = createEntryStore(spaceId);
  const [showCreateEntryDialog, setShowCreateEntryDialog] = createSignal(false);
  const [showCreateFormDialog, setShowCreateFormDialog] = createSignal(false);

  const [space] = createResource(async () => {
    return await spaceApi.get(spaceId());
  });

  const [forms, { refetch: refetchForms }] = createResource(
    () => spaceId(),
    async (wsId) => {
      if (!wsId) return [];
      return await formApi.list(wsId);
    },
  );

  const [columnTypes] = createResource(
    () => spaceId(),
    async (wsId) => {
      if (!wsId) return [];
      return await formApi.listTypes(wsId);
    },
  );

  const safeForms = createMemo(() => forms() || []);
  const entryForms = createMemo(() => filterCreatableEntryForms(safeForms()));
  const hasCreatableForms = createMemo(() => entryForms().length > 0);
  const needsFirstFormGuidance = createMemo(() =>
    !forms.loading && !hasCreatableForms()
  );
  const defaultEntryForm = createMemo(() => {
    const settings = space()?.settings;
    const configured = settings && typeof settings === "object"
      ? settings.default_form
      : undefined;
    if (typeof configured === "string") {
      const trimmed = configured.trim();
      if (
        trimmed && entryForms().some((entryForm) => entryForm.name === trimmed)
      ) {
        return trimmed;
      }
    }
    return entryForms()[0]?.name;
  });
  const displaySpaceName = createMemo(() => space()?.name || spaceId());

  const handleCreateForm = async (payload: FormCreatePayload) => {
    await formApi.create(spaceId(), payload);
    setShowCreateFormDialog(false);
    await refetchForms();
  };

  const handleCreateEntry = async (
    title: string,
    formName: string,
    requiredValues: Record<string, string>,
    inputMode: EntryInputMode = "webform",
  ) => {
    if (!formName) {
      throw new Error(t("dashboard.error.selectFormBeforeCreate"));
    }
    const formDef = entryForms().find((entryForm) =>
      entryForm.name === formName
    );
    if (!formDef) {
      throw new Error(t("dashboard.error.selectedFormNotFound"));
    }
    const initialContent = buildEntryMarkdownByMode(
      formDef,
      title,
      requiredValues,
      inputMode,
    );
    const result = await entryStore.createEntry(initialContent);
    setShowCreateEntryDialog(false);
    navigate(`/spaces/${spaceId()}/entries/${encodeURIComponent(result.id)}`);
  };

  return (
    <SpaceShell spaceId={spaceId()} activeNavigation="home">
      <div class="mx-auto max-w-5xl ui-stack">
        <div>
          <p class="text-sm ui-muted">{displaySpaceName()}</p>
          <h1 class="ui-page-title text-3xl sm:text-4xl">Home</h1>
          <Show when={space.error}>
            <p class="text-sm ui-text-danger">
              {t("dashboard.error.failedLoadSpace")}
            </p>
          </Show>
        </div>

        <section class="ui-card ui-stack-sm">
          <div class="flex flex-wrap items-start justify-between gap-3">
            <div>
              <h2 class="text-lg font-semibold">Continue</h2>
              <p class="text-sm ui-muted">Start an entry from a form, or return to a form workspace.</p>
            </div>
            <Show when={needsFirstFormGuidance()}>
              <div class="ui-alert ui-alert-warning text-sm ui-stack-sm">
                <div class="ui-stack-sm">
                  <p class="font-medium">
                    {t("dashboard.section.createEntry.empty")}
                  </p>
                  <p>
                    {t("dashboard.section.createEntry.firstFormDescription")}
                  </p>
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
            <div class="flex flex-wrap gap-2">
              <button
                type="button"
                class="ui-button text-sm"
                classList={{
                  "ui-button-primary": hasCreatableForms(),
                  "ui-button-secondary": !hasCreatableForms(),
                }}
                disabled={!hasCreatableForms()}
                onClick={() => setShowCreateEntryDialog(true)}
              >
                {t("dashboard.section.createEntry.new")}
              </button>
              <A
                href={`/spaces/${spaceId()}/forms`}
                class="ui-button ui-button-secondary text-sm"
              >
                {t("dashboard.section.createEntry.browse")}
              </A>
            </div>
          </div>
        </section>
        <div class="grid gap-4 sm:grid-cols-2">
          <section class="ui-card ui-stack-sm">
            <h2 class="text-lg font-semibold">Pinned</h2>
            <p class="text-sm ui-muted">Pin the entries and views you return to often.</p>
            <A href={`/spaces/${spaceId()}/forms`} class="ui-button ui-button-secondary text-sm">Browse forms</A>
          </section>
          <section class="ui-card ui-stack-sm">
            <h2 class="text-lg font-semibold">Recent</h2>
            <p class="text-sm ui-muted">Your latest entries will appear here as you work.</p>
            <Show when={hasCreatableForms()}>
              <button
                type="button"
                class="ui-button ui-button-primary text-sm"
                onClick={() => setShowCreateEntryDialog(true)}
              >
                {t("dashboard.section.createEntry.new")}
              </button>
            </Show>
          </section>
        </div>
      </div>

      <CreateEntryDialog
        open={showCreateEntryDialog()}
        forms={entryForms()}
        spaceId={spaceId()}
        defaultForm={defaultEntryForm()}
        onClose={() => setShowCreateEntryDialog(false)}
        onSubmit={handleCreateEntry}
      />
      <CreateFormDialog
        open={showCreateFormDialog()}
        columnTypes={columnTypes() || []}
        formNames={safeForms().map((form) => form.name)}
        onClose={() => setShowCreateFormDialog(false)}
        onSubmit={handleCreateForm}
      />
    </SpaceShell>
  );
}
