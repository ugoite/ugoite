import { A, useParams } from "@solidjs/router";
import { createResource, For, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { UiIcon } from "~/components/UiIcon";
import { entryApi } from "~/lib/ugoite-client";

export default function SpaceEntryHistoryRoute() {
  const params = useParams<{ space_id: string; entry_id: string }>();
  const spaceId = () => params.space_id;
  const entryId = () => params.entry_id;
  const encodedEntryId = () => encodeURIComponent(entryId());
  const [history] = createResource(() =>
    entryApi.history(spaceId(), entryId())
  );

  return (
    <SpaceShell
      spaceId={spaceId()}
      activeNavigation="forms"
      title="Entry / History"
    >
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{entryId()}</div>
          <h1>History</h1>
        </div>
        <A
          href={`/spaces/${spaceId()}/entries/${encodedEntryId()}`}
          class="btn"
        >
          Back to Entry
        </A>
      </div>
      <Show when={history.loading}>
        <p class="ui-muted">Loading history...</p>
      </Show>
      <Show when={history.error}>
        <p class="ui-alert ui-alert-error">Failed to load history.</p>
      </Show>
      <Show when={history()}>
        {(data) => (
          <div class="rowStack">
            <For each={data().revisions}>
              {(revision) => (
                <A
                  class="rowBtn"
                  href={`/spaces/${spaceId()}/entries/${encodedEntryId()}/history/${
                    encodeURIComponent(revision.revision_id)
                  }`}
                >
                  <span class="glyph active">
                    <UiIcon name="history" />
                  </span>
                  <span>
                    <b>{revision.revision_id}</b>
                    <small>{revision.created_at}</small>
                  </span>
                  <span>›</span>
                </A>
              )}
            </For>
          </div>
        )}
      </Show>
    </SpaceShell>
  );
}
