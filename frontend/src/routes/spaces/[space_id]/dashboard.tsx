import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createSignal, For, onMount, Show } from "solid-js";
import { CreateFormDialog } from "~/components/create-dialogs";
import { SpaceShell } from "~/components/SpaceShell";
import { UiIcon } from "~/components/UiIcon";
import { createEntryStore } from "~/lib/entry-store";
import { getDocsiteHref } from "~/lib/docsite-links";
import { t } from "~/lib/i18n";
import { createResource } from "~/lib/recoverable-resource";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import { formApi, spaceApi } from "~/lib/ugoite-client";
import type { FormCreatePayload } from "~/lib/types";

const browserWalkthroughUrl = getDocsiteHref(
  "/docs/guide/start/browser-first-entry",
  "docs/guide/start/browser-first-entry.md",
);

export default function SpaceDashboardRoute() {
  const params = useParams<{ space_id: string }>();
  const navigate = useNavigate();
  const spaceId = () => params.space_id;
  const entryStore = createEntryStore(spaceId);
  const [showFormDialog, setShowFormDialog] = createSignal(false);
  const [entriesLoaded, setEntriesLoaded] = createSignal(false);
  const [space] = createResource(spaceId, spaceApi.get);
  const [forms, { refetch: refetchForms }] = createResource(
    spaceId,
    formApi.list,
  );
  const [columnTypes] = createResource(spaceId, formApi.listTypes);
  const entryForms = createMemo(() => filterCreatableEntryForms(forms() ?? []));
  const formsAvailable = () => !forms.loading && !forms.error;
  const spaceName = () => space()?.name || spaceId();
  const storeEntries = () => {
    const value = entryStore.entries as unknown;
    return typeof value === "function"
      ? (value as () => ReturnType<typeof entryStore.entries>)()
      : (value as ReturnType<typeof entryStore.entries>);
  };
  const recentEntries = createMemo(() =>
    [...storeEntries()].sort((a, b) =>
      String(b.updated_at).localeCompare(String(a.updated_at))
    ).slice(0, 4)
  );
  const isFreshSpace = createMemo(() =>
    entriesLoaded() && !entryStore.error() && recentEntries().length === 0
  );

  onMount(() => {
    void entryStore.loadEntries().then(() => setEntriesLoaded(true));
  });

  const createForm = async (payload: FormCreatePayload) => {
    await formApi.create(spaceId(), payload);
    setShowFormDialog(false);
    await refetchForms();
  };
  const startNewEntry = () => {
    if (!formsAvailable()) return;
    if (entryForms().length) {
      navigate(`/spaces/${spaceId()}/entries/new`);
    } else {
      setShowFormDialog(true);
    }
  };

  return (
    <SpaceShell
      spaceId={spaceId()}
      activeNavigation="home"
      title={t("dashboard.home")}
    >
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{spaceName()}</div>
          <h1>{t("dashboard.home")}</h1>
        </div>
        <button
          class="btn primary"
          type="button"
          disabled={!formsAvailable()}
          onClick={startNewEntry}
        >
          <UiIcon name="plus" /> {t("dashboard.newEntry")}
        </button>
      </div>

      <Show when={forms.error}>
        <section class="surface emptyState" role="alert">
          <p>{t("dashboard.formsLoadFailed")}</p>
          <button class="btn" type="button" onClick={() => void refetchForms()}>
            {t("dashboard.retry")}
          </button>
        </section>
      </Show>

      <section class="section">
        <div class="sectionHead">
          <h2>{t("dashboard.continue")}</h2>
        </div>
        <div class="grid3">
          <Show
            when={recentEntries()[0]}
            fallback={
              <div class="card ui-stack-sm">
                <button
                  class="cardBtn"
                  type="button"
                  disabled={!formsAvailable()}
                  onClick={startNewEntry}
                >
                  <span class="glyph active">
                    <UiIcon name="entry" />
                  </span>
                  <span>
                    <b>{t("dashboard.newEntry")}</b>
                    <small>{t("dashboard.noRecent")}</small>
                  </span>
                  <span class="chev">›</span>
                </button>
                <Show when={isFreshSpace()}>
                  <a
                    class="ui-muted text-sm hover:underline"
                    href={browserWalkthroughUrl}
                    target="_blank"
                    rel="noopener"
                  >
                    {t("dashboard.walkthrough")}
                  </a>
                </Show>
              </div>
            }
          >
            {(entry) => (
              <A
                class="card cardBtn"
                href={`/spaces/${spaceId()}/entries/${
                  encodeURIComponent(entry().id)
                }`}
              >
                <span class="glyph active">
                  <UiIcon name="entry" />
                </span>
                <span>
                  <b>{entry().title || t("common.untitled")}</b>
                  <small>{entry().form || t("dashboard.entry")}</small>
                </span>
                <span class="chev">›</span>
              </A>
            )}
          </Show>
          <A class="card cardBtn" href={`/spaces/${spaceId()}/forms`}>
            <span class="glyph">
              {entryForms()[0]?.name?.slice(0, 1).toUpperCase() || "F"}
            </span>
            <span>
              <b>{entryForms()[0]?.name || t("dashboard.forms")}</b>
              <small>{t("dashboard.formsEntries")}</small>
            </span>
            <span class="chev">›</span>
          </A>
          <A class="card cardBtn" href={`/spaces/${spaceId()}/search`}>
            <span class="glyph">
              <UiIcon name="search" />
            </span>
            <span>
              <b>{t("dashboard.search")}</b>
              <small>{t("dashboard.searchMeta")}</small>
            </span>
            <span class="chev">›</span>
          </A>
        </div>
      </section>

      <section class="section">
        <div class="sectionHead">
          <h2>{t("dashboard.pinned")}</h2>
        </div>
        <div class="grid4">
          <For each={entryForms().slice(0, 2)}>
            {(form) => (
              <A
                class="tile"
                href={`/spaces/${spaceId()}/forms?form=${
                  encodeURIComponent(form.name)
                }`}
              >
                <span class="glyph">{form.name.slice(0, 1).toUpperCase()}</span>
                <span>
                  <b>{form.name}</b>
                  <small>{t("dashboard.form")}</small>
                </span>
              </A>
            )}
          </For>
          <A class="tile" href={`/spaces/${spaceId()}/assets`}>
            <span class="glyph">
              <UiIcon name="asset" />
            </span>
            <span>
              <b>{t("dashboard.assets")}</b>
              <small>{t("dashboard.search")}</small>
            </span>
          </A>
          <A class="tile" href={`/spaces/${spaceId()}/sql`}>
            <span class="glyph">
              <UiIcon name="sql" />
            </span>
            <span>
              <b>{t("dashboard.savedSql")}</b>
              <small>{t("dashboard.saved")}</small>
            </span>
          </A>
        </div>
      </section>

      <section class="section">
        <div class="sectionHead">
          <h2>{t("dashboard.recent")}</h2>
        </div>
        <div class="rowStack">
          <For
            each={recentEntries()}
            fallback={
              <div class="rowBtn">
                <span class="glyph">
                  <UiIcon name="entry" />
                </span>
                <span>
                  <b>{t("dashboard.noRecent")}</b>
                  <small>{spaceName()}</small>
                </span>
              </div>
            }
          >
            {(entry) => (
              <A
                class="rowBtn"
                href={`/spaces/${spaceId()}/entries/${
                  encodeURIComponent(entry.id)
                }`}
              >
                <span class="glyph">
                  <UiIcon name="entry" />
                </span>
                <span>
                  <b>{entry.title || t("common.untitled")}</b>
                  <small>{t("dashboard.entry")} · {entry.form || "—"}</small>
                </span>
                <span>›</span>
              </A>
            )}
          </For>
        </div>
      </section>

      <CreateFormDialog
        open={showFormDialog()}
        columnTypes={columnTypes() ?? []}
        formNames={(forms() ?? []).map((form) => form.name)}
        onClose={() => setShowFormDialog(false)}
        onSubmit={createForm}
      />
    </SpaceShell>
  );
}
