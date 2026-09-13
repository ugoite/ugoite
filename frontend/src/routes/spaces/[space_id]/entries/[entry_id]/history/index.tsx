import { A, useParams } from "@solidjs/router";
import { createEffect, createSignal, For, Show } from "solid-js";
import { UiIcon } from "~/components/UiIcon";
import { formatDateTimeLabel } from "~/lib/date-format";
import { entryApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { spaceRoute } from "~/lib/space-shell-route";
import { pageFromArray } from "~/lib/pagination";
import type { EntryRevision } from "~/lib/types";
import { t } from "~/lib/i18n";

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

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{entryId()}</div>
          <h1>History</h1>
        </div>
        <A
          href={`/spaces/${spaceId()}/entries/${encodedEntryId()}`}
          class="btn"
        >
          Back to Entry
        </A>
        <A href={`/spaces/${spaceId()}/history`} class="btn">
          View space history
        </A>
      </div>
      <Show when={history.loading}>
        <p class="ui-muted">{t("entryHistory.loading")}</p>
      </Show>
      <Show when={history.error}>
        <p class="ui-alert ui-alert-error">Failed to load history.</p>
      </Show>
      <Show when={loadMoreError()}>
        <p class="ui-alert ui-alert-error">{loadMoreError()}</p>
      </Show>
      <Show when={history()}>
        {() => (
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
                  <span>
                    <b>{revision.revision_id}</b>
                    <small>{formatDateTimeLabel(revision.timestamp)}</small>
                  </span>
                  <span>›</span>
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
        )}
      </Show>
    </>
  );
}
