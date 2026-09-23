import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createSignal, For, onMount, Show } from "solid-js";
import { CreateFormDialog } from "~/components/create-dialogs";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { UiIcon } from "~/components/UiIcon";
import { createEntryStore } from "~/lib/entry-store";
import { entryDisplayLabel } from "~/lib/entry-label";
import { getDocsiteHref } from "~/lib/docsite-links";
import { t } from "~/lib/i18n";
import { createResource } from "~/lib/recoverable-resource";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import { formApi, spaceApi } from "~/lib/ugoite-client";
import type { FormCreatePayload } from "~/lib/types";
import {
  spaceEntriesPath,
  spaceEntryPath,
  spaceFormEntriesPath,
  spaceFormsPath,
  spaceSearchPath,
  spaceSqlPath,
} from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "home" });

const browserWalkthroughUrl = getDocsiteHref(
  "/docs/get-started/quickstart",
  "docs/get-started/quickstart.mdx",
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
  const formReadiness = createMemo<"loading" | "failed" | "empty" | "ready">(
    () => {
      if (forms.loading) return "loading";
      if (forms.error) return "failed";
      return entryForms().length > 0 ? "ready" : "empty";
    },
  );
  const formsAvailable = () =>
    formReadiness() === "ready" ||
    formReadiness() === "empty";
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
      navigate(spaceEntriesPath(spaceId(), "/new"));
    } else {
      setShowFormDialog(true);
    }
  };

  return (
    <>
      <h1 class="ui-sr-only">{t("dashboard.home")}</h1>
      <div class="homehead">
        <div class="actionLead">
          <span class="eyebrow">{spaceName()}</span>
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

      <Show when={formReadiness() === "loading"}>
        <section
          class="surface emptyState"
          aria-live="polite"
          aria-busy="true"
        >
          {/* Spinner only: label is sr-only for assistive technology. */}
          <LocalBusyIndicator
            label={t("dashboard.section.createEntry.loading")}
          />
        </section>
      </Show>

      <Show when={formReadiness() === "empty"}>
        <section class="surface emptyState ui-stack-sm">
          <p>{t("dashboard.section.createEntry.empty")}</p>
          <p class="ui-muted">
            {t("dashboard.section.createEntry.firstFormDescription")}
          </p>
          <button
            class="btn"
            type="button"
            onClick={() => setShowFormDialog(true)}
          >
            {t("dashboard.section.createEntry.createFirstForm")}
          </button>
        </section>
      </Show>

      <section class="section">
        <div class="sectionHead">
          <h2>{t("dashboard.continue")}</h2>
        </div>
        <div class="continueGrid">
          <Show
            when={recentEntries()[0]}
            fallback={
              <div class="continueItem continueItemEmpty">
                <button
                  class="continueItemButton"
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
                class="continueItem"
                href={spaceEntryPath(spaceId(), entry().id)}
              >
                <span class="glyph active">
                  <UiIcon name="entry" />
                </span>
                <span>
                  <b>{entryDisplayLabel(entry())}</b>
                  <small>{entry().form || t("dashboard.entry")}</small>
                </span>
                <span class="chev">›</span>
              </A>
            )}
          </Show>
          <A class="continueItem" href={spaceFormsPath(spaceId())}>
            <span class="glyph">
              {entryForms()[0]?.name?.slice(0, 1).toUpperCase() || "F"}
            </span>
            <span>
              <b>{entryForms()[0]?.name || t("dashboard.forms")}</b>
              <small>{t("dashboard.formsEntries")}</small>
            </span>
            <span class="chev">›</span>
          </A>
          <A class="continueItem" href={spaceSearchPath(spaceId())}>
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
        <div class="pinGrid">
          <For each={entryForms().slice(0, 2)}>
            {(form) => (
              <A
                class="pinItem"
                href={spaceFormEntriesPath(spaceId(), form.name)}
              >
                <span class="glyph">{form.name.slice(0, 1).toUpperCase()}</span>
                <span>
                  <b>{form.name}</b>
                  <small>{t("dashboard.form")}</small>
                </span>
              </A>
            )}
          </For>
          <A class="pinItem" href={spaceSqlPath(spaceId())}>
            <span class="glyph">
              <UiIcon name="sql" />
            </span>
            <span>
              <b>{t("sqlPage.savedSql")}</b>
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
                </span>
              </div>
            }
          >
            {(entry) => (
              <A
                class="rowBtn"
                href={spaceEntryPath(spaceId(), entry.id)}
              >
                <span class="glyph">
                  <UiIcon name="entry" />
                </span>
                <span>
                  <b>{entryDisplayLabel(entry)}</b>
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
    </>
  );
}
