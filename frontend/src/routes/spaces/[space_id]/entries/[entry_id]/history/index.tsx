import { A, useParams } from "@solidjs/router";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { formatDateTimeLabel } from "~/lib/date-format";
import { revisionActor, revisionOperationLabel } from "~/lib/entry-history";
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
  const encodedSpaceId = () => encodeURIComponent(spaceId());
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
      ? formatUserFacingError(
        history.error,
        "entryHistory.loadError",
        "entry.history",
      )
      : null
  );

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{entryId()}</div>
          <h1>{t("entryHistory.title")}</h1>
        </div>
        <BackLink
          href={`/spaces/${encodedSpaceId()}/entries/${encodedEntryId()}`}
          label={t("entryHistory.backToEntry")}
        />
      </div>
      {
        /* Panel-local spinner only: existing rows stay mounted during refetch,
          no visible loading text. */
      }
      <Show when={history.loading}>
        <LocalBusyIndicator label={t("entryHistory.loading")} />
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
            {
              /* Only this wrapper scrolls horizontally; the page itself never
                does. Three columns only: operation / actor / timestamp plus
                a chevron. Revision id, title, form, and summary stay hidden
                (the revision route owns that detail). */
            }
            <div class="tablewrap" aria-busy={history.loading || undefined}>
              <table class="dataTable entry-history-table">
                <thead>
                  <tr>
                    <th scope="col">{t("entryHistory.operation")}</th>
                    <th scope="col">{t("entryHistory.actor")}</th>
                    <th scope="col">{t("entryHistory.timestamp")}</th>
                    <th scope="col">
                      <span class="ui-sr-only">{t("entryHistory.title")}</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  <For each={revisions()}>
                    {(revision) => (
                      <tr>
                        <td>
                          <A
                            class="table-link"
                            href={`/spaces/${encodedSpaceId()}/entries/${encodedEntryId()}/history/${
                              encodeURIComponent(revision.revision_id)
                            }`}
                          >
                            {revisionOperationLabel(revision)}
                          </A>
                        </td>
                        <td class="ui-muted">{revisionActor(revision)}</td>
                        <td class="ui-muted">
                          {formatDateTimeLabel(revision.timestamp)}
                        </td>
                        <td aria-hidden="true">
                          <span class="chev">›</span>
                        </td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
            <Show when={hasMore()}>
              <button
                type="button"
                class="ui-button ui-button-secondary"
                disabled={loadingMore()}
                onClick={() => void loadMoreHistory()}
              >
                {t("entryHistory.loadMore")}
              </button>
              {/* Footer spinner only: existing rows stay visible. */}
              <Show when={loadingMore()}>
                <LocalBusyIndicator
                  size="sm"
                  label={t("entryHistory.loadingMore")}
                />
              </Show>
            </Show>
          </Show>
        )}
      </Show>
    </>
  );
}
