import { useNavigate, useSearchParams } from "@solidjs/router";
import {
  createEffect,
  createMemo,
  createResource,
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
import { assetApi, formApi } from "~/lib/ugoite-client";
import type { FormCreatePayload } from "~/lib/types";

type FormTab = "entries" | "fields" | "assets" | "views";
const tabLabels: Record<FormTab, { en: string; ja: string }> = {
  entries: { en: "Entries", ja: "エントリー" },
  fields: { en: "Fields", ja: "フィールド" },
  assets: { en: "Assets", ja: "アセット" },
  views: { en: "Views", ja: "ビュー" },
};
const copy = {
  en: {
    forms: "Forms",
    newForm: "Form",
    find: "Find a Form",
    noForms: "No Forms yet",
    select: "Select a Form",
    edit: "Edit Form",
    newEntry: "Entry",
    linkedAssets: "Assets linked to entries in this Form",
    viewsText: "Saved views for this Form will appear here.",
  },
  ja: {
    forms: "フォーム",
    newForm: "フォーム",
    find: "フォームを探す",
    noForms: "フォームがありません",
    select: "フォームを選択",
    edit: "フォームを編集",
    newEntry: "エントリー",
    linkedAssets: "このフォームのエントリーに紐づくアセット",
    viewsText: "このフォームの保存済みビューがここに表示されます。",
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
  const activeTab = createMemo<FormTab>(() =>
    ["fields", "assets", "views"].includes(String(params.tab))
      ? String(params.tab) as FormTab
      : "entries"
  );
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
  const [assets] = createResource(
    () => activeTab() === "assets" ? ctx.spaceId() : null,
    async (spaceId) => spaceId ? assetApi.list(spaceId) : [],
  );

  createEffect(() => {
    if (!selectedForm() && forms()[0]) {
      setParams({ form: forms()[0].name, tab: activeTab() }, { replace: true });
    }
  });

  const createForm = async (payload: FormCreatePayload) => {
    await formApi.create(ctx.spaceId(), payload);
    setShowFormDialog(false);
    ctx.refetchForms();
    setParams({ form: payload.name, tab: "entries" });
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
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{ctx.spaceId()}</div>
          <h1>{c().forms}</h1>
        </div>
        <button
          class="btn"
          type="button"
          onClick={() => setShowFormDialog(true)}
        >
          <UiIcon name="plus" /> {c().newForm}
        </button>
      </div>
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
                onClick={() => setParams({ form: form.name, tab: activeTab() })}
              >
                <span
                  class="glyph"
                  classList={{ active: selectedName() === form.name }}
                >
                  {form.name.slice(0, 1).toUpperCase()}
                </span>
                <span>
                  <b>{form.name}</b>
                  <small>{Object.keys(form.fields).join(" · ") || "—"}</small>
                  <span class="miniFields">
                    <For each={Object.values(form.fields).slice(0, 3)}>
                      {(field) => <span>{field.type}</span>}
                    </For>
                  </span>
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
                <div class="contextBar">
                  <div class="contextLeft">
                    <span class="glyph active">
                      {form().name.slice(0, 1).toUpperCase()}
                    </span>
                    <span>
                      <b>{form().name}</b>
                      <small>{c().forms} / {form().name}</small>
                    </span>
                  </div>
                  <button
                    class="btn iconBtn"
                    type="button"
                    aria-label={c().edit}
                    onClick={() => setShowEditDialog(true)}
                  >
                    ⚙
                  </button>
                </div>
                <div class="tabs" role="tablist">
                  <For
                    each={["entries", "fields", "assets", "views"] as FormTab[]}
                  >
                    {(tab) => (
                      <button
                        class="tab"
                        classList={{ active: activeTab() === tab }}
                        type="button"
                        role="tab"
                        aria-selected={activeTab() === tab}
                        onClick={() => setParams({ form: form().name, tab })}
                      >
                        {tabLabels[tab][locale() === "ja" ? "ja" : "en"]}
                      </button>
                    )}
                  </For>
                </div>
                <Show when={activeTab() === "entries"}>
                  <div class="toolbar">
                    <div class="toolbarLeft">
                      <span class="ui-muted">
                        Forms / {form().name} / Entries
                      </span>
                    </div>
                    <div class="toolbarRight">
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
                </Show>
                <Show when={activeTab() === "fields"}>
                  <div class="rowStack">
                    <For
                      each={Object.entries(form().fields)}
                      fallback={
                        <div class="surface settingsMain ui-muted">
                          No fields
                        </div>
                      }
                    >
                      {([name, field]) => (
                        <div class="rowBtn">
                          <span class="glyph">#</span>
                          <span>
                            <b>{name}</b>
                            <small>
                              {field.type}
                              {field.required ? " · required" : ""}
                            </small>
                          </span>
                          <span class="pill">{field.type}</span>
                        </div>
                      )}
                    </For>
                  </div>
                </Show>
                <Show when={activeTab() === "assets"}>
                  <p class="ui-muted mb-3">{c().linkedAssets}</p>
                  <div class="assetList">
                    <For
                      each={assets() ?? []}
                      fallback={
                        <div class="surface settingsMain ui-muted">
                          No linked assets
                        </div>
                      }
                    >
                      {(asset) => (
                        <a
                          class="assetRow"
                          href={`/spaces/${ctx.spaceId()}/assets/${
                            encodeURIComponent(asset.id)
                          }`}
                        >
                          <span class="fileIcon">
                            {asset.name?.split(".").pop()?.toUpperCase() ||
                              "FILE"}
                          </span>
                          <span>
                            <b>{asset.name || asset.id}</b>
                            <small>{asset.path || "Asset"}</small>
                          </span>
                          <span>›</span>
                        </a>
                      )}
                    </For>
                  </div>
                </Show>
                <Show when={activeTab() === "views"}>
                  <div class="surface settingsMain ui-muted">
                    {c().viewsText}
                  </div>
                </Show>
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
