import { useNavigate, useParams } from "@solidjs/router";
import { Show } from "solid-js";
import { EntryDetailPane } from "~/components/EntryDetailPane";
import { useEntriesRouteContext } from "~/lib/entries-route-context";
import { t } from "~/lib/i18n";

export default function SpaceEntryDetailRoute() {
  const navigate = useNavigate();
  const ctx = useEntriesRouteContext();
  const params = useParams<{ space_id: string; entry_id: string }>();
  const spaceId = () => params.space_id || "";
  // SolidJS router already decodes URL parameters
  const entryId = () => params.entry_id ?? "";

  return (
    <>
      <div class="mx-auto max-w-6xl">
        <Show
          when={!ctx.loadingForms() && !ctx.formsError?.()}
          fallback={
            <div class="ui-entry-page">
              <Show
                when={ctx.formsError?.()}
                fallback={
                  <div class="ui-card text-center">
                    <p class="ui-muted text-sm">{t("entryDetail.loading")}</p>
                  </div>
                }
              >
                <div class="ui-card text-center space-y-3">
                  <p class="ui-alert ui-alert-error">
                    {t("formsPage.failedLoad")}
                  </p>
                  <button
                    type="button"
                    class="ui-button ui-button-secondary"
                    onClick={() => void ctx.refetchForms()}
                  >
                    {t("formsPage.retry")}
                  </button>
                </div>
              </Show>
            </div>
          }
        >
          <EntryDetailPane
            spaceId={spaceId}
            entryId={entryId}
            forms={ctx.forms}
            onDeleted={() => {
              navigate(`/spaces/${spaceId()}/forms`, { replace: true });
            }}
          />
        </Show>
      </div>
    </>
  );
}
