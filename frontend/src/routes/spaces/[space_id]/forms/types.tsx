import { A, useParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { formApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
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
          <div class="eyebrow">Forms</div>
          <h1>Form Field Types</h1>
        </div>
        <A href={`/spaces/${spaceId()}/forms`} class="btn">
          Back to Forms
        </A>
      </div>

      <Show when={types.loading}>
        <p class="ui-muted">Loading types...</p>
      </Show>
      <Show when={types.error}>
        <p class="ui-alert ui-alert-error">Failed to load form types.</p>
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
                    <small>Field type</small>
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
