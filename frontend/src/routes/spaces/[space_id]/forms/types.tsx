import { useParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { formApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms" });

export default function SpaceFormTypesRoute() {
  const params = useParams<{ space_id: string }>();
  const spaceId = () => params.space_id;

  const [types] = createResource(async () => {
    return await formApi.listTypes(spaceId());
  });

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <h1>{t("formTypesPage.heading")}</h1>
        </div>
      </div>

      {/* Panel-local spinner: loaded types stay mounted on refetch. */}
      <Show when={types.loading}>
        <LocalBusyIndicator label={t("formTypesPage.loading")} />
      </Show>
      <Show when={types.error}>
        <p class="ui-alert ui-alert-error">{t("formTypesPage.failedLoad")}</p>
      </Show>
      <Show when={types()}>
        {(list) => (
          <div class="grid3">
            <For each={list()}>
              {(item) => (
                <div class="tile">
                  <span class="glyph">Aa</span>
                  <b>{item}</b>
                </div>
              )}
            </For>
          </div>
        )}
      </Show>
    </>
  );
}
