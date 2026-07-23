import { A, useParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { UiIcon } from "~/components/UiIcon";
import { sqlApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";

export default function SpaceSqlIndexRoute() {
  const params = useParams<{ space_id: string }>();
  const spaceId = () => params.space_id;
  const [queries] = createResource(spaceId, sqlApi.list);

  return (
    <SpaceShell spaceId={spaceId()} activeNavigation="search" title="Saved SQL">
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">Search</div>
          <h1>Saved SQL</h1>
        </div>
        <A class="btn primary" href={`/spaces/${spaceId()}/queries/new`}>
          <UiIcon name="plus" /> SQL
        </A>
      </div>
      <Show when={queries.loading}>
        <p class="ui-muted">Loading saved SQL...</p>
      </Show>
      <Show when={queries.error}>
        <p class="ui-alert ui-alert-error">Failed to load saved SQL.</p>
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
                <b>No saved SQL</b>
                <small>Create a query to reuse it here.</small>
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
                <b>{query.name}</b>
                <small>{query.updated_at}</small>
              </span>
              <span>›</span>
            </A>
          )}
        </For>
      </div>
    </SpaceShell>
  );
}
