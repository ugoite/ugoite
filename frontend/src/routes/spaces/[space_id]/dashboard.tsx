import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, For, onMount, Show } from "solid-js";
import { CreateFormDialog } from "~/components/create-dialogs";
import { SpaceShell } from "~/components/SpaceShell";
import { UiIcon } from "~/components/UiIcon";
import { createEntryStore } from "~/lib/entry-store";
import { locale } from "~/lib/i18n";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import { formApi, spaceApi } from "~/lib/ugoite-client";
import type { FormCreatePayload } from "~/lib/types";

const copy = {
  en: { home: "Home", newEntry: "Entry", continue: "Continue", pinned: "Pinned", recent: "Recent", forms: "Forms", search: "Search", assets: "Assets", savedSql: "SQL", noRecent: "Create an entry to start building this Space.", form: "Form", entry: "Entry", searchMeta: "Entries · Forms · Assets" },
  ja: { home: "ホーム", newEntry: "エントリー", continue: "続きから", pinned: "ピン留め", recent: "最近", forms: "フォーム", search: "検索", assets: "アセット", savedSql: "SQL", noRecent: "エントリーを作成して、このスペースを育てましょう。", form: "フォーム", entry: "エントリー", searchMeta: "エントリー · フォーム · アセット" },
} as const;

export default function SpaceDashboardRoute() {
  const params = useParams<{ space_id: string }>();
  const navigate = useNavigate();
  const spaceId = () => params.space_id;
  const entryStore = createEntryStore(spaceId);
  const [showFormDialog, setShowFormDialog] = createSignal(false);
  const c = () => copy[locale() === "ja" ? "ja" : "en"];

  const [space] = createResource(spaceId, spaceApi.get);
  const [forms, { refetch: refetchForms }] = createResource(spaceId, formApi.list);
  const [columnTypes] = createResource(spaceId, formApi.listTypes);
  const entryForms = createMemo(() => filterCreatableEntryForms(forms() ?? []));
  const spaceName = () => space()?.name || spaceId();
  const storeEntries = () => {
    const value = entryStore.entries as unknown;
    return typeof value === "function"
      ? (value as () => ReturnType<typeof entryStore.entries>)()
      : (value as ReturnType<typeof entryStore.entries>);
  };
  const recentEntries = createMemo(() => [...storeEntries()].sort((a, b) => String(b.updated_at).localeCompare(String(a.updated_at))).slice(0, 4));

  onMount(() => void entryStore.loadEntries());

  const createForm = async (payload: FormCreatePayload) => {
    await formApi.create(spaceId(), payload);
    setShowFormDialog(false);
    await refetchForms();
  };

  return (
    <SpaceShell spaceId={spaceId()} activeNavigation="home" title={c().home}>
      <div class="screenHead">
        <div class="screenTitle"><div class="eyebrow">{spaceName()}</div><h1>{c().home}</h1></div>
        <button class="btn primary" type="button" onClick={() => entryForms().length ? navigate(`/spaces/${spaceId()}/entries/new`) : setShowFormDialog(true)}>
          <UiIcon name="plus" /> {c().newEntry}
        </button>
      </div>

      <section class="section">
        <div class="sectionHead"><h2>{c().continue}</h2></div>
        <div class="grid3">
          <Show when={recentEntries()[0]} fallback={
            <button class="card cardBtn" type="button" onClick={() => entryForms().length ? navigate(`/spaces/${spaceId()}/entries/new`) : setShowFormDialog(true)}>
              <span class="glyph active"><UiIcon name="entry" /></span><span><b>{c().newEntry}</b><small>{c().noRecent}</small></span><span class="chev">›</span>
            </button>
          }>
            {(entry) => <A class="card cardBtn" href={`/spaces/${spaceId()}/entries/${encodeURIComponent(entry().id)}`}><span class="glyph active"><UiIcon name="entry" /></span><span><b>{entry().title || "Untitled"}</b><small>{entry().form || c().entry}</small></span><span class="chev">›</span></A>}
          </Show>
          <A class="card cardBtn" href={`/spaces/${spaceId()}/forms`}><span class="glyph">{entryForms()[0]?.name?.slice(0,1).toUpperCase() || "F"}</span><span><b>{entryForms()[0]?.name || c().forms}</b><small>{c().forms} / Entries</small></span><span class="chev">›</span></A>
          <A class="card cardBtn" href={`/spaces/${spaceId()}/search`}><span class="glyph"><UiIcon name="search" /></span><span><b>{c().search}</b><small>{c().searchMeta}</small></span><span class="chev">›</span></A>
        </div>
      </section>

      <section class="section">
        <div class="sectionHead"><h2>{c().pinned}</h2></div>
        <div class="grid4">
          <For each={entryForms().slice(0, 2)}>{(form) => <A class="tile" href={`/spaces/${spaceId()}/forms?form=${encodeURIComponent(form.name)}`}><span class="glyph">{form.name.slice(0,1).toUpperCase()}</span><span><b>{form.name}</b><small>{c().form}</small></span></A>}</For>
          <A class="tile" href={`/spaces/${spaceId()}/assets`}><span class="glyph"><UiIcon name="asset" /></span><span><b>{c().assets}</b><small>{c().search}</small></span></A>
          <A class="tile" href={`/spaces/${spaceId()}/sql`}><span class="glyph"><UiIcon name="sql" /></span><span><b>{c().savedSql}</b><small>Saved</small></span></A>
        </div>
      </section>

      <section class="section">
        <div class="sectionHead"><h2>{c().recent}</h2></div>
        <div class="rowStack">
          <For each={recentEntries()} fallback={<div class="rowBtn"><span class="glyph"><UiIcon name="entry" /></span><span><b>{c().noRecent}</b><small>{spaceName()}</small></span></div>}>
            {(entry) => <A class="rowBtn" href={`/spaces/${spaceId()}/entries/${encodeURIComponent(entry.id)}`}><span class="glyph"><UiIcon name="entry" /></span><span><b>{entry.title || "Untitled"}</b><small>{c().entry} · {entry.form || "—"}</small></span><span>›</span></A>}
          </For>
        </div>
      </section>

      <CreateFormDialog open={showFormDialog()} columnTypes={columnTypes() ?? []} formNames={(forms() ?? []).map((form) => form.name)} onClose={() => setShowFormDialog(false)} onSubmit={createForm} />
    </SpaceShell>
  );
}
