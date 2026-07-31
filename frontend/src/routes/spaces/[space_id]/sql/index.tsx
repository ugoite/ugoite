import { A, useParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { UiIcon } from "~/components/UiIcon";
import { sqlApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { displaySqlName } from "~/lib/sql-metadata";
import { formatDateLabel } from "~/lib/date-format";
import { formatUserFacingError } from "~/lib/user-facing-error";

export default function SpaceSqlIndexRoute() {
  const params = useParams<{ space_id: string }>();
  const spaceId = () => params.space_id;
  const [queries] = createResource(spaceId, sqlApi.list);

  return (
    <SpaceShell
      spaceId={spaceId()}
      activeNavigation="search"
      title={t("sqlPage.savedSql")}
    >
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{t("searchPage.title")}</div>
          <h1>{t("sqlPage.savedSql")}</h1>
        </div>
        <A class="btn primary" href={`/spaces/${spaceId()}/queries/new`}>
          <UiIcon name="plus" /> {t("sqlPage.createButton")}
        </A>
      </div>
      <Show when={queries.loading}>
        <p class="ui-muted">{t("sqlPage.loadingSavedSql")}</p>
      </Show>
      <Show when={queries.error}>
        <p class="ui-alert ui-alert-error">
          {formatUserFacingError(
            queries.error,
            "sqlPage.failedLoadSavedSql",
          )}
        </p>
      </Show>
      <div class="rowStack">
        <For
          each={queries() ?? []}
          fallback={
            <div class="rowBtn">
              <span class="glyph">
                <UiIcon name="sql" />
              </span>
              <span>
                <b>{t("sqlPage.noSavedSql")}</b>
                <small>{t("sqlPage.createDescription")}</small>
              </span>
            </div>
          }
        >
          {(query) => (
            <A
              class="rowBtn"
              href={`/spaces/${spaceId()}/sql/${encodeURIComponent(query.id)}`}
            >
              <span class="glyph active">
                <UiIcon name="sql" />
              </span>
              <span>
                <b>{displaySqlName(query)}</b>
                <small>{formatDateLabel(query.updated_at)}</small>
              </span>
              <span>›</span>
            </A>
          )}
        </For>
      </div>
    </SpaceShell>
  );
}
