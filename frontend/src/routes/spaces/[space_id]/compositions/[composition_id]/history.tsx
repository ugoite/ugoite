import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import { BackLink } from "~/components/BackLink";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { RowList, RowListItem, RowListLink } from "~/components/RowList";
import { formatDateTimeLabel } from "~/lib/date-format";
import { t } from "~/lib/i18n";
import {
  compositionApi,
  compositionDisplayName,
  type CompositionHistoryPage,
  type CompositionRawRevision,
} from "~/lib/composition-api";
import {
  spaceCompositionRevisionPath,
  spaceCompositionsPath,
} from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "home" });

const PAGE_SIZE = 50;

type LoadState =
  | { status: "loading" }
  | { status: "ready"; page: CompositionHistoryPage }
  | { status: "error" };

/**
 * Minimal operation label for one history row. The server reports `upsert`,
 * `delete`, or `restore`; history arrives oldest first, so the first global
 * revision is the creation and later writes are edits.
 */
export const compositionHistoryOperationLabel = (
  revision: CompositionRawRevision,
  isFirst: boolean,
): string => {
  if (revision.revision.operation === "restore") {
    return t("composition.operation.restored");
  }
  if (revision.revision.operation !== "delete" && isFirst) {
    return t("composition.operation.created");
  }
  return t("composition.operation.edited");
};

const revisionTimestamp = (revision: CompositionRawRevision): string =>
  formatDateTimeLabel(revision.revision.committed_at_micros / 1000);

export default function CompositionHistoryRoute() {
  const params = useParams<{ space_id: string; composition_id: string }>();
  const spaceId = () => params.space_id;
  const compositionId = () => params.composition_id;
  const [offset, setOffset] = createSignal(0);
  const [retry, setRetry] = createSignal(0);
  const [loadState, setLoadState] = createSignal<LoadState>({
    status: "loading",
  });
  const [currentRevisionId, setCurrentRevisionId] = createSignal<
    string | undefined
  >(undefined);
  let generation = 0;
  let controller: AbortController | undefined;

  createEffect(() => {
    const requestedSpace = spaceId();
    const requestedComposition = compositionId();
    const requestedOffset = offset();
    retry();
    const requestGeneration = ++generation;
    controller?.abort();
    const currentController = new AbortController();
    controller = currentController;
    setLoadState({ status: "loading" });
    void compositionApi.history(
      requestedSpace,
      requestedComposition,
      PAGE_SIZE,
      requestedOffset,
      currentController.signal,
    ).then(
      (page) => {
        if (
          requestGeneration === generation && !currentController.signal.aborted
        ) {
          setLoadState({ status: "ready", page });
        }
      },
      () => {
        if (
          requestGeneration === generation && !currentController.signal.aborted
        ) {
          setLoadState({ status: "error" });
        }
      },
    );
  });

  // The current revision comes from the bounded list projection; history
  // rows never carry identity beyond their own revision.
  createEffect(() => {
    const requestedSpace = spaceId();
    const requestedComposition = compositionId();
    retry();
    void compositionApi.list(requestedSpace, 100, 0).then(
      (listed) => {
        setCurrentRevisionId(
          listed.items.find(
            (item) => item.composition_id === requestedComposition,
          )?.revision_id,
        );
      },
      () => {
        setCurrentRevisionId(undefined);
      },
    );
  });

  onCleanup(() => {
    generation += 1;
    controller?.abort();
  });

  const page = () => {
    const current = loadState();
    return current.status === "ready" ? current.page : undefined;
  };
  const name = () => compositionDisplayName(page()?.revisions[0]?.fields?.name);

  return (
    <section class="section">
      <h1>{name()}</h1>
      <BackLink
        href={spaceCompositionsPath(spaceId())}
        label={t("composition.historyBack")}
      />
      <Show when={loadState().status === "loading"}>
        <LocalBusyIndicator label={t("composition.historyLoading")} />
      </Show>
      <Show when={loadState().status === "error"}>
        <p class="ui-text-danger" role="alert">
          {t("composition.historyFailed")}
        </p>
        <button
          class="ui-button ui-button-secondary"
          type="button"
          onClick={() => setRetry((value) => value + 1)}
        >
          {t("composition.retry")}
        </button>
      </Show>
      <Show when={page()}>
        {(currentPage) => (
          <>
            <Show
              when={currentPage().revisions.length > 0}
              fallback={<p class="ui-muted">{t("composition.historyEmpty")}</p>}
            >
              <RowList
                label={t("composition.history")}
                labelledBy={undefined}
              >
                <For each={currentPage().revisions}>
                  {(revision, index) => {
                    const globalIndex = () => currentPage().offset + index();
                    const operation = () =>
                      compositionHistoryOperationLabel(
                        revision,
                        globalIndex() === 0,
                      );
                    const isCurrent = () =>
                      currentRevisionId() !== undefined &&
                      revision.revision.revision_id === currentRevisionId();
                    const rowLabel = () =>
                      `${operation()} · ${revisionTimestamp(revision)}${
                        isCurrent()
                          ? ` · ${t("composition.historyCurrent")}`
                          : ""
                      }`;
                    return (
                      <RowListItem
                        main={
                          <RowListLink
                            href={spaceCompositionRevisionPath(
                              spaceId(),
                              compositionId(),
                              revision.revision.revision_id,
                            )}
                            primary={operation()}
                            secondary={isCurrent()
                              ? t("composition.historyCurrent")
                              : undefined}
                            meta={revisionTimestamp(revision)}
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
              <details>
                <summary>{t("composition.technicalDetails")}</summary>
                <dl>
                  <For each={currentPage().revisions}>
                    {(revision) => (
                      <div>
                        <dt>{t("composition.revisionId")}</dt>
                        <dd class="font-mono break-all">
                          {revision.revision.revision_id}
                        </dd>
                      </div>
                    )}
                  </For>
                </dl>
              </details>
            </Show>
            <nav
              class="rowListActions mt-4"
              aria-label={t("composition.history")}
            >
              <button
                class="ui-button ui-button-secondary"
                type="button"
                disabled={currentPage().offset === 0}
                onClick={() => setOffset(
                  Math.max(0, currentPage().offset - currentPage().limit),
                )}
              >
                {t("composition.previous")}
              </button>
              <button
                class="ui-button ui-button-secondary"
                type="button"
                disabled={!currentPage().has_more}
                onClick={() =>
                  setOffset(currentPage().offset + currentPage().limit)}
              >
                {t("composition.next")}
              </button>
            </nav>
          </>
        )}
      </Show>
    </section>
  );
}
