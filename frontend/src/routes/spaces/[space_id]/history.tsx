import { A, useParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { UiIcon } from "~/components/UiIcon";
import { formatDateTimeLabel } from "~/lib/date-format";
import { changeApi, type SpaceChange } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "home", title: "spaceHistory" });

const changeKind = (change: SpaceChange): string =>
  change.reverts_change_id
    ? t("spaceHistory.revert")
    : t("spaceHistory.change");

export default function SpaceHistoryRoute() {
  const params = useParams<{ space_id: string }>();
  const spaceId = () => params.space_id;
  const [history] = createResource(() => changeApi.list(spaceId()));

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{spaceId()}</div>
          <h1>{t("spaceHistory.title")}</h1>
        </div>
        <A href={`/spaces/${spaceId()}/dashboard`} class="btn">
          {t("spaceHistory.backToSpace")}
        </A>
      </div>
      <p class="ui-muted">{t("spaceHistory.description")}</p>
      <Show when={history.loading}>
        <p class="ui-muted">{t("spaceHistory.loading")}</p>
      </Show>
      <Show when={history.error}>
        <p class="ui-alert ui-alert-error">{t("spaceHistory.loadError")}</p>
      </Show>
      <Show when={history()}>
        {(data) => (
          <Show
            when={data().length > 0}
            fallback={<p class="ui-muted">{t("spaceHistory.empty")}</p>}
          >
            <div class="rowStack">
              <For each={data()}>
                {(change) => (
                  <div class="ui-card">
                    <span class="glyph active">
                      <UiIcon name="history" />
                    </span>
                    <span>
                      <b>{changeKind(change)}</b>
                      <Show when={change.message}>
                        <span>{change.message}</span>
                      </Show>
                      <small>
                        {formatDateTimeLabel(change.created_at_micros / 1000)}
                        {" · "}
                        {change.actor_principal_id}
                      </small>
                      <small>
                        {t("spaceHistory.changeId", {
                          value: change.change_id,
                        })}
                      </small>
                      <Show when={change.run_id}>
                        <small>
                          {t("spaceHistory.runId", {
                            value: change.run_id ?? "",
                          })}
                        </small>
                      </Show>
                    </span>
                  </div>
                )}
              </For>
            </div>
          </Show>
        )}
      </Show>
    </>
  );
}
