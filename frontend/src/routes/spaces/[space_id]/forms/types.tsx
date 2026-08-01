import { A, useParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { formApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms", title: "formTypes" });

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
          <div class="eyebrow">{t("spaceShell.bottom.grid")}</div>
          <h1>{t("formTypesPage.heading")}</h1>
        </div>
        <A href={`/spaces/${spaceId()}/forms`} class="btn">
          {t("formTypesPage.back")}
        </A>
      </div>

      <Show when={types.loading}>
        <p class="ui-muted">{t("formTypesPage.loading")}</p>
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
                  <span>
                    <b>{item}</b>
                    <small>{t("formTypesPage.fieldType")}</small>
                  </span>
                </div>
              )}
            </For>
          </div>
        )}
      </Show>
    </>
  );
}
