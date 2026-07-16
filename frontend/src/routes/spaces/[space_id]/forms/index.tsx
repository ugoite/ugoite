import { useNavigate, useSearchParams } from "@solidjs/router";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  Show,
} from "solid-js";
import { CreateFormDialog, EditFormDialog } from "~/components/create-dialogs";
import { FormTable } from "~/components/FormTable";
import { SpaceShell } from "~/components/SpaceShell";
import { UiIcon } from "~/components/UiIcon";
import { useEntriesRouteContext } from "~/lib/entries-route-context";
import { locale } from "~/lib/i18n";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import { formApi } from "~/lib/ugoite-client";
import type { FormCreatePayload } from "~/lib/types";

const copy = {
  en: {
    forms: "Forms",
    newForm: "Form",
    find: "Find a Form",
    noForms: "No Forms yet",
    select: "Select a Form",
    edit: "Edit Form",
    newEntry: "Entry",
  },
  ja: {
    forms: "フォーム",
    newForm: "フォーム",
    find: "フォームを探す",
    noForms: "フォームがありません",
    select: "フォームを選択",
    edit: "フォームを編集",
    newEntry: "エントリー",
  },
} as const;

export default function SpaceFormsIndexPane() {
  const ctx = useEntriesRouteContext();
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const [query, setQuery] = createSignal("");
  const [showFormDialog, setShowFormDialog] = createSignal(false);
  const [showEditDialog, setShowEditDialog] = createSignal(false);
  const c = () => copy[locale() === "ja" ? "ja" : "en"];
  const forms = createMemo(() => filterCreatableEntryForms(ctx.forms()));
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
      title={c().forms}
    >
      <div class="split">
        <aside class="listPane surface">
          <div class="paneHead">
            <b>{c().forms}</b>
            <button
              class="btn iconBtn"
              type="button"
              aria-label={c().newForm}
              onClick={() => setShowFormDialog(true)}
            >
              <UiIcon name="plus" />
            </button>
          </div>
          <label class="miniSearch">
            <UiIcon name="search" />
            <input
              value={query()}
              onInput={(event) => setQuery(event.currentTarget.value)}
              placeholder={c().find}
            />
          </label>
          <For
            each={filteredForms()}
            fallback={<div class="ui-muted p-3">{c().noForms}</div>}
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
              <div class="surface settingsMain ui-muted">{c().select}</div>
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
                      <UiIcon name="settings" /> {c().edit}
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
                        <UiIcon name="plus" /> {c().newEntry}
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
    </SpaceShell>
  );
}
