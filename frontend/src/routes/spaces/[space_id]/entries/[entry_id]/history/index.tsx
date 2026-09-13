import { A, useParams } from "@solidjs/router";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { UiIcon } from "~/components/UiIcon";
import { formatDateTimeLabel } from "~/lib/date-format";
import {
  revisionActor,
  revisionForm,
  revisionOperationLabel,
  revisionSummary,
  revisionTitle,
} from "~/lib/entry-history";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { t } from "~/lib/i18n";
import { entryApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { spaceRoute } from "~/lib/space-shell-route";
import { pageFromArray } from "~/lib/pagination";
import type { EntryRevision } from "~/lib/types";

export const route = spaceRoute({ navigation: "forms", title: "entryHistory" });

const HISTORY_PAGE_SIZE = 50;

export default function SpaceEntryHistoryRoute() {
  const params = useParams<{ space_id: string; entry_id: string }>();
  const spaceId = () => params.space_id;
  const entryId = () => params.entry_id;
  const encodedEntryId = () => encodeURIComponent(entryId());
  const [history] = createResource(() =>
    entryApi.history(spaceId(), entryId(), undefined, HISTORY_PAGE_SIZE + 1)
  );
  const [revisions, setRevisions] = createSignal<EntryRevision[]>([]);
  const [hasMore, setHasMore] = createSignal(false);
  const [loadingMore, setLoadingMore] = createSignal(false);
  const [loadMoreError, setLoadMoreError] = createSignal<string | null>(null);

  createEffect(() => {
    const data = history();
    if (!data) return;
    const page = pageFromArray(data.revisions, HISTORY_PAGE_SIZE);
    setRevisions(page.items);
    setHasMore(page.hasMore);
  });

  const loadMoreHistory = async () => {
    if (loadingMore() || !hasMore()) return;
    setLoadingMore(true);
    setLoadMoreError(null);
    try {
      const data = await entryApi.history(
        spaceId(),
        entryId(),
        undefined,
        HISTORY_PAGE_SIZE + 1,
        revisions().length,
      );
      const page = pageFromArray(data.revisions, HISTORY_PAGE_SIZE);
      setRevisions((current) => [...current, ...page.items]);
      setHasMore(page.hasMore);
    } catch {
      setLoadMoreError(t("entryHistory.failedLoadMore"));
    } finally {
      setLoadingMore(false);
    }
  };
  const errorMessage = createMemo(() =>
    history.error
      ? formatUserFacingError(history.error, "entryHistory.loadError", "entry.history")
      : null
  );

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{entryId()}</div>
          <h1>{t("entryHistory.title")}</h1>
        </div>
        <A
          href={`/spaces/${spaceId()}/entries/${encodedEntryId()}`}
          class="btn"
        >
          {t("entryHistory.backToEntry")}
        </A>
        <A href={`/spaces/${spaceId()}/history`} class="btn">
          {t("entryHistory.viewSpaceHistory")}
        </A>
      </div>
      <Show when={history.loading}>
        <p class="ui-muted">{t("entryHistory.loading")}</p>
      </Show>
      <Show when={errorMessage()}>
        <p class="ui-alert ui-alert-error">{errorMessage()}</p>
      </Show>
      <Show when={loadMoreError()}>
        <p class="ui-alert ui-alert-error">{loadMoreError()}</p>
      </Show>
      <Show when={history()}>
        {(data) => (
          <Show
            when={data().revisions.length > 0}
            fallback={<p class="ui-muted">{t("entryHistory.empty")}</p>}
          >
            <div class="rowStack">
              <For each={revisions()}>
                {(revision) => (
                  <A
                    class="rowBtn"
                    href={`/spaces/${spaceId()}/entries/${encodedEntryId()}/history/${
                      encodeURIComponent(revision.revision_id)
                    }`}
                  >
                    <span class="glyph active">
                      <UiIcon name="history" />
                    </span>
                    <span class="ui-stack-sm">
                      <span>
                        <strong>{revisionOperationLabel(revision)}</strong>
                        <span class="ui-muted"> · {revisionSummary(revision)}</span>
                      </span>
                      <span class="ui-entry-history-meta">
                        <span>
                          <b>{t("entryHistory.actor")}:</b> {revisionActor(revision)}
                        </span>
                        <span>
                          <b>{t("entryHistory.timestamp")}:</b>{" "}
                          {formatDateTimeLabel(revision.timestamp)}
                        </span>
                      </span>
                      <span class="ui-entry-history-meta">
                        <span>
                          <b>{t("common.title")}:</b> {revisionTitle(revision)}
                        </span>
                        <span>
                          <b>{t("common.form")}:</b> {revisionForm(revision)}
                        </span>
                      </span>
                      <small class="ui-muted">
                        {t("entryHistory.revisionId")}: {revision.revision_id}
                      </small>
                    </span>
                    <span aria-hidden="true">›</span>
                  </A>
                )}
              </For>
              <Show when={hasMore()}>
                <button
                  type="button"
                  class="ui-button ui-button-secondary"
                  disabled={loadingMore()}
                  onClick={() => void loadMoreHistory()}
                >
                  {loadingMore()
                    ? t("entryHistory.loadingMore")
                    : t("entryHistory.loadMore")}
                </button>
              </Show>
            </div>
          </Show>
        )}
      </Show>
    </>
  );
}
