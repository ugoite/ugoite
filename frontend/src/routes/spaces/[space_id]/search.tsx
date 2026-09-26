import { useNavigate, useParams, useSearchParams } from "@solidjs/router";
import {
  createEffect,
  createMemo,
  createSignal,
  onMount,
  Show,
  untrack,
} from "solid-js";
import { A } from "@solidjs/router";
import { UiIcon } from "~/components/UiIcon";
import { EntryBrowser } from "~/components/EntryBrowser";
import { formApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import {
  createEntryQueryController,
  systemEntryCapabilities,
} from "~/lib/entry-query";
import { spaceEntryPath, spaceSqlPath } from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";
import { t } from "~/lib/i18n";

export const route = spaceRoute({ navigation: "search" });

/**
 * Canonical Search preset (PR1).
 *
 * Collection reads go through EntryQuery only: this route owns a disposable
 * EntryQueryController scoped to All Forms with an optional text query. All
 * filter/sort/operator decisions render from the Rust-owned capability
 * descriptor; no local operator tables exist here.
 */
export default function SpaceSearchRoute() {
  const params = useParams<{ space_id: string }>();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const spaceId = () => params.space_id;
  const [formMetadata, { refetch: refetchForms }] = createResource(
    spaceId,
    async (requestedSpaceId) => ({
      spaceId: requestedSpaceId,
      forms: await formApi.list(requestedSpaceId),
    }),
  );
  const formLabelsState = createMemo<"loading" | "ready" | "error">(() => {
    if (formMetadata.state === "errored") return "error";
    const metadata = formMetadata();
    if (
      formMetadata.state !== "ready" || !metadata ||
      metadata.spaceId !== spaceId()
    ) return "loading";
    return "ready";
  });
  const formLabels = createMemo(() => {
    const metadata = formMetadata();
    if (
      formLabelsState() !== "ready" || !metadata ||
      metadata.spaceId !== spaceId()
    ) return undefined;
    return Object.fromEntries(
      metadata.forms.filter((form) => form.id).map((form) => [
        form.id!,
        form.name,
      ]),
    );
  });
  const initialText = () => {
    const raw = searchParams.q;
    const text = Array.isArray(raw) ? raw[0] : raw;
    return typeof text === "string" ? text.trim() : "";
  };

  const scope = createMemo(() => ({ kind: "all" as const }));
  const capabilities = createMemo(() => systemEntryCapabilities(scope()));
  // Draft/commit separation: the text field owns a raw draft signal while
  // typing. Only submit commits the normalized value into EntryQuery.
  // `?q=` is an initialization/explicit-navigation input only; committed
  // query state never flows back into the draft.
  const [draftText, setDraftText] = createSignal(initialText());
  const controller = createEntryQueryController(
    spaceId,
    {
      scope: scope(),
      ...(initialText() ? { text: initialText() } : {}),
      filters: [],
      sort: [],
    },
    { kind: "preview" },
  );

  let lastUrlText = initialText();
  onMount(() => {
    // A deep link carries the first search condition; run it once instead
    // of waiting for an explicit submit. Empty mounts stay idle so the
    // initial guidance remains visible until the user searches.
    if (lastUrlText) void controller.load();
  });
  createEffect(() => {
    const urlText = initialText();
    if (urlText === lastUrlText) return;
    lastUrlText = urlText;
    setDraftText(urlText);
    const committed = untrack(() => controller.query().text ?? "");
    if (urlText !== committed) controller.setText(urlText);
  });

  const commitDraft = () => {
    // Commit boundary: normalize once so the field and the committed
    // query agree after submit.
    const committed = draftText().trim();
    setDraftText(committed);
    controller.setText(committed);
  };

  return (
    <div class="searchWorkspace">
      <h1 class="ui-sr-only" id="search-page-title">
        {t("searchPage.title")}
      </h1>
      <div class="ui-muted">
        <A href={spaceSqlPath(spaceId())}>{t("searchPage.nav.saved")}</A>
      </div>
      <section class="searchControls" aria-labelledby="search-page-title">
        <form
          class="mt-5 flex flex-col gap-3 sm:flex-row sm:items-center"
          onSubmit={(event) => {
            event.preventDefault();
            commitDraft();
          }}
        >
          <div class="flex-1">
            <label class="ui-sr-only" for="search-keywords">
              {t("searchPage.searchKeywords")}
            </label>
            <div class="searchBox">
              <input
                id="search-keywords"
                type="text"
                placeholder={t("searchPage.keywordPlaceholder")}
                value={draftText()}
                onInput={(event) => setDraftText(event.currentTarget.value)}
              />
            </div>
          </div>
          <div class="sm:self-end queryLane">
            <button
              type="submit"
              class="ui-button ui-button-primary text-sm"
              aria-label={t("searchPage.searchEntries")}
            >
              <UiIcon name="search" />
            </button>
          </div>
        </form>
      </section>
      <Show when={formLabelsState() === "loading"}>
        <p class="mt-3 ui-muted" aria-live="polite">
          {t("searchPage.loadingFormNames")}
        </p>
      </Show>
      <Show when={formLabelsState() === "error"}>
        <div class="mt-3 flex flex-wrap items-center gap-2">
          <p class="ui-text-danger" role="alert">
            {t("searchPage.failedLoadFormNames")}
          </p>
          <button
            type="button"
            class="ui-button ui-button-secondary"
            disabled={formMetadata.loading}
            onClick={() => void refetchForms()}
          >
            {t("common.retry")}
          </button>
        </div>
      </Show>
      <EntryBrowser
        controller={controller}
        capabilities={capabilities()}
        formLabels={formLabels()}
        formLabelsState={formLabelsState()}
        onSelect={(row) => navigate(spaceEntryPath(spaceId(), row.id))}
      />
    </div>
  );
}
