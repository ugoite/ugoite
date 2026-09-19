import { useParams } from "@solidjs/router";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { RowList, RowListItem, RowListLink } from "~/components/RowList";
import { formatDateTimeLabel } from "~/lib/date-format";
import {
  actorDisplayNameLookup,
  resolveActorDisplayName,
  revisionOperationLabel,
} from "~/lib/entry-history";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { t } from "~/lib/i18n";
import { entryApi, spaceApi } from "~/lib/ugoite-client";
import type { SpaceMember } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import { spaceEntryPath } from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";
import { pageFromArray } from "~/lib/pagination";
import type { EntryRevision } from "~/lib/types";

export const route = spaceRoute({
  navigation: "entries",
  title: "entryHistory",
});

const HISTORY_PAGE_SIZE = 50;

export default function SpaceEntryHistoryRoute() {
  const params = useParams<{ space_id: string; entry_id: string }>();
  const spaceId = () => params.space_id;
  const entryId = () => params.entry_id;
  const entryHref = () => spaceEntryPath(spaceId(), entryId());
  const revisionHref = (revisionId: string) =>
    `${spaceEntryPath(spaceId(), entryId())}/history/${
      encodeURIComponent(revisionId)
    }`;
  const [history] = createResource(() =>
    entryApi.history(spaceId(), entryId(), undefined, HISTORY_PAGE_SIZE + 1)
  );
  // Best-effort member directory for actor display names. The API carries
  // only opaque actor identity strings on revisions; when the directory is
  // unavailable (or the actor left), rows fall back to the stable short
  // form — never a raw UUID.
  const [members] = createResource(
    () => spaceId(),
    async (id): Promise<SpaceMember[]> => {
      try {
        return await spaceApi.listMembers(id);
      } catch {
        return [];
      }
    },
    { initialValue: [] as SpaceMember[] },
  );
  const actorLookup = createMemo(() =>
    actorDisplayNameLookup(
      (members() ?? []).map((member) => ({
        principal_id: member.principal.principal_id,
        display_name: member.principal.display_name,
      })),
    )
  );
  const actorName = (revision: EntryRevision) =>
    resolveActorDisplayName(revision, actorLookup());
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
          <h1>{t("entryHistory.title")}</h1>
        </div>
        <BackLink
          href={entryHref()}
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
              /*
              RowList rows (PR4): the full row activates to the revision
              review (click or Enter on the native link). Primary is the
              operation, secondary the actor display name, meta the compact
              timestamp, plus an unboxed chevron. Revision id, title, form,
              and raw actor UUIDs stay out of rows (the revision route owns
              that detail).
            */
            }
            <div aria-busy={history.loading || undefined}>
              <RowList label={t("entryHistory.title")}>
                <For each={revisions()}>
                  {(revision) => {
                    const operation = () => revisionOperationLabel(revision);
                    const name = () => actorName(revision);
                    const when = () => formatDateTimeLabel(revision.timestamp);
                    const rowLabel = () =>
                      `${operation()} · ${name()} · ${when()}`;
                    return (
                      <RowListItem
                        main={
                          <RowListLink
                            href={revisionHref(revision.revision_id)}
                            primary={operation()}
                            secondary={name()}
                            meta={when()}
                            chevron
                            ariaLabel={rowLabel()}
                            title={rowLabel()}
                          />
                        }
                      />
                    );
                  }}
                </For>
              </RowList>
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
