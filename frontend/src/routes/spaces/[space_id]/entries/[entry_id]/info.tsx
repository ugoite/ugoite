import { useParams } from "@solidjs/router";
import { createMemo, Show } from "solid-js";
import { AccessPolicyEditor } from "~/components/AccessPolicyEditor";
import { BackLink } from "~/components/BackLink";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { formatDateTimeLabel } from "~/lib/date-format";
import { t } from "~/lib/i18n";
import { entryApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceEntryPath } from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms" });

export default function SpaceEntryInfoRoute() {
  const params = useParams<{ space_id: string; entry_id: string }>();
  const spaceId = () => params.space_id;
  const entryId = () => params.entry_id;
  const entryPath = () => spaceEntryPath(spaceId(), entryId());

  const [entry] = createResource(
    () => spaceId() && entryId() ? `${spaceId()}/${entryId()}` : null,
    () => entryApi.get(spaceId(), entryId()),
  );

  const errorMessage = createMemo(() =>
    entry.error
      ? formatUserFacingError(entry.error, "entryInfo.loadError", "entry.get")
      : null
  );

  const copyEntryId = async () => {
    try {
      await navigator.clipboard.writeText(entryId());
    } catch {
      // Clipboard is a progressive enhancement; the ID stays visible.
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow break-all">
            {entryId()}
            <button
              type="button"
              class="ui-button ui-button-secondary ui-button-sm ml-2"
              aria-label={`${t("common.copy")} ${entryId()}`}
              title={t("common.copy")}
              onClick={() => void copyEntryId()}
            >
              {t("common.copy")}
            </button>
          </div>
          <h1>{t("entryInfo.title")}</h1>
        </div>
        <BackLink href={entryPath()} label={t("entryInfo.backToEntry")} />
      </div>
      {/* Panel-local spinner: rendered info stays mounted on refetch. */}
      <Show when={entry.loading}>
        <LocalBusyIndicator label={t("entryInfo.loading")} />
      </Show>
      <Show when={errorMessage()}>
        <p class="ui-alert ui-alert-error">{errorMessage()}</p>
      </Show>
      <Show when={!entry.loading && !entry.error && !entry()}>
        <p class="ui-muted">{t("entryInfo.notFound")}</p>
      </Show>
      <Show when={entry()}>
        {(loaded) => (
          <dl
            class="ui-entry-detail-list ui-entry-info-list"
            aria-busy={entry.loading || undefined}
          >
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
