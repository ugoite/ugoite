import { createEffect, createSignal, onCleanup, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import { CompositionRows } from "~/components/CompositionRows";
import { IconLink } from "~/components/IconLink";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { t } from "~/lib/i18n";
import {
  compositionApi,
  type CompositionListPage,
} from "~/lib/composition-api";
import { spaceCompositionNewPath } from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "compositions" });
const PAGE_SIZE = 50;

type LoadState =
  | { status: "loading" }
  | { status: "ready"; page: CompositionListPage }
  | { status: "error" };

export default function CompositionListRoute() {
  const params = useParams<{ space_id: string }>();
  const spaceId = () => params.space_id;
  const [offset, setOffset] = createSignal(0);
  const [retry, setRetry] = createSignal(0);
  const [loadState, setLoadState] = createSignal<LoadState>({
    status: "loading",
  });
  let generation = 0;
  let controller: AbortController | undefined;

  createEffect(() => {
    const requestedSpace = spaceId();
    const requestedOffset = offset();
    retry();
    const requestGeneration = ++generation;
    controller?.abort();
    const currentController = new AbortController();
    controller = currentController;
    setLoadState({ status: "loading" });
    void compositionApi.list(
      requestedSpace,
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

  onCleanup(() => {
    generation += 1;
    controller?.abort();
  });

  const page = () => {
    const current = loadState();
    return current.status === "ready" ? current.page : undefined;
  };

  return (
    <section class="section">
      <div class="flex items-center gap-2">
        <h1>{t("composition.listHeading")}</h1>
        <IconLink
          icon="plus"
          label={t("composition.new")}
          href={spaceCompositionNewPath(spaceId())}
        />
      </div>
      <Show when={loadState().status === "loading"}>
        <LocalBusyIndicator label={t("composition.listLoading")} />
      </Show>
      <Show when={loadState().status === "error"}>
        <p class="ui-text-danger" role="alert">
          {t("composition.listFailed")}
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
              when={currentPage().items.length > 0}
              fallback={<p class="ui-muted">{t("composition.listEmpty")}</p>}
            >
              <CompositionRows
                spaceId={spaceId()}
                items={currentPage().items}
                label={t("composition.listHeading")}
              />
            </Show>
            <nav
              class="rowListActions mt-4"
              aria-label={t("composition.listHeading")}
            >
              <button
                class="ui-button ui-button-secondary"
                type="button"
                disabled={currentPage().offset === 0}
                onClick={() =>
                  setOffset(
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
