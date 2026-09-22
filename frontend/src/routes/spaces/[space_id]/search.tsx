import { useNavigate, useParams, useSearchParams } from "@solidjs/router";
import { createEffect, createMemo } from "solid-js";
import { A } from "@solidjs/router";
import { EntryBrowser } from "~/components/EntryBrowser";
import {
  createEntryQueryController,
  systemEntryCapabilities,
} from "~/lib/entry-query";
import { spaceAssetsPath, spaceEntryPath, spaceSqlPath } from "~/lib/space-path";
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
  const initialText = () => {
    const raw = searchParams.q;
    const text = Array.isArray(raw) ? raw[0] : raw;
    return typeof text === "string" ? text.trim() : "";
  };

  const scope = createMemo(() => ({ kind: "all" as const }));
  const capabilities = createMemo(() => systemEntryCapabilities(scope()));
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

  createEffect(() => {
    const text = initialText();
    const current = controller.query().text ?? "";
    if (text !== current) controller.setText(text);
  });

  return (
    <div class="searchWorkspace">
      <h1 class="ui-sr-only" id="search-page-title">
        {t("searchPage.title")}
      </h1>
      <nav class="ui-muted" aria-label={t("searchPage.title")}>
        <A href={spaceAssetsPath(spaceId())}>{t("searchPage.nav.files")}</A>
        {" · "}
        <A href={spaceSqlPath(spaceId())}>{t("searchPage.nav.saved")}</A>
      </nav>
      <p class="text-sm ui-muted">{t("searchPage.savedSqlHint")}</p>
      <section class="searchControls" aria-labelledby="search-page-title">
        <form
          class="mt-5 flex flex-col gap-3 sm:flex-row sm:items-center"
          onSubmit={(event) => {
            event.preventDefault();
            const input = event.currentTarget.querySelector(
              "#search-keywords",
            ) as HTMLInputElement | null;
            controller.setText(input?.value ?? "");
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
                value={controller.query().text ?? ""}
                onInput={(event) =>
                  controller.setText(event.currentTarget.value)}
              />
            </div>
          </div>
          <div class="sm:self-end queryLane">
            <button type="submit" class="ui-button ui-button-primary text-sm">
              {t("searchPage.searchEntries")}
            </button>
          </div>
        </form>
      </section>
      <EntryBrowser
        controller={controller}
        capabilities={capabilities()}
        onSelect={(row) => navigate(spaceEntryPath(spaceId(), row.id))}
      />
    </div>
  );
}
