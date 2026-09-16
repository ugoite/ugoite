import { A, useParams } from "@solidjs/router";
import { createMemo, Show } from "solid-js";
import { AccessPolicyEditor } from "~/components/AccessPolicyEditor";
import { formatDateTimeLabel } from "~/lib/date-format";
import { t } from "~/lib/i18n";
import { entryApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms", title: "entryInfo" });

export default function SpaceEntryInfoRoute() {
  const params = useParams<{ space_id: string; entry_id: string }>();
  const spaceId = () => params.space_id;
  const entryId = () => params.entry_id;
  const encodedEntryId = () => encodeURIComponent(entryId());
  const entryPath = () =>
    `/spaces/${spaceId()}/entries/${encodedEntryId()}`;

  const [entry] = createResource(
    () => spaceId() && entryId() ? `${spaceId()}/${entryId()}` : null,
    () => entryApi.get(spaceId(), entryId()),
  );

  const errorMessage = createMemo(() =>
    entry.error
      ? formatUserFacingError(entry.error, "entryInfo.loadError", "entry.get")
      : null
  );

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{entryId()}</div>
          <h1>{t("entryInfo.title")}</h1>
        </div>
        <A href={entryPath()} class="btn">
          {t("entryInfo.backToEntry")}
        </A>
      </div>
      <Show when={entry.loading}>
        <p class="ui-muted">{t("entryInfo.loading")}</p>
      </Show>
      <Show when={errorMessage()}>
        <p class="ui-alert ui-alert-error">{errorMessage()}</p>
      </Show>
      <Show when={!entry.loading && !entry.error && !entry()}>
        <p class="ui-muted">{t("entryInfo.notFound")}</p>
      </Show>
      <Show when={entry()}>
        {(loaded) => (
          <dl class="ui-entry-detail-list ui-entry-info-list">
            <div>
              <dt>{t("entryInfo.entryId")}</dt>
              <dd class="font-mono break-all">{loaded().id}</dd>
            </div>
            <div>
              <dt>{t("entryInfo.form")}</dt>
              <dd>{loaded().form || t("entryInfo.unknownForm")}</dd>
            </div>
            <div>
              <dt>{t("entryInfo.updatedAt")}</dt>
              <dd>{formatDateTimeLabel(loaded().updated_at)}</dd>
            </div>
            <Show when={loaded().created_at}>
              <div>
                <dt>{t("entryInfo.createdAt")}</dt>
                <dd>{formatDateTimeLabel(loaded().created_at)}</dd>
              </div>
            </Show>
            <Show when={loaded().revision_id}>
              <div>
                <dt>{t("entryInfo.revisionId")}</dt>
                <dd class="font-mono break-all">{loaded().revision_id}</dd>
              </div>
            </Show>
          </dl>
        )}
      </Show>
      <Show when={entry()}>
        <section aria-label={t("entryInfo.sharing")}>
          <AccessPolicyEditor
            spaceId={spaceId()}
            kind="entry"
            resourceId={entryId()}
          />
        </section>
      </Show>
    </>
  );
}
