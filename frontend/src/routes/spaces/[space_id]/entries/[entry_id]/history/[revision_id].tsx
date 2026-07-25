import { A, useParams } from "@solidjs/router";
import { Show } from "solid-js";
import { entryApi } from "~/lib/ugoite-client";
import { SpaceShell } from "~/components/SpaceShell";
import { createResource } from "~/lib/recoverable-resource";

export default function SpaceEntryRevisionRoute() {
  const params = useParams<
    { space_id: string; entry_id: string; revision_id: string }
  >();
  const spaceId = () => params.space_id;
  const entryId = () => params.entry_id;
  const revisionId = () => params.revision_id;

  const [revision] = createResource(async () => {
    return await entryApi.getRevision(spaceId(), entryId(), revisionId());
  });

  return (
    <SpaceShell spaceId={spaceId()} activeNavigation="forms" title="Revision">
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{entryId()} / History</div>
          <h1>Revision</h1>
        </div>
        <A
          href={`/spaces/${spaceId()}/entries/${
            encodeURIComponent(entryId())
          }/history`}
          class="btn"
        >
          Back to History
        </A>
      </div>

      <Show when={revision.loading}>
        <p class="ui-muted">Loading revision...</p>
      </Show>
      <Show when={revision.error}>
        <p class="ui-alert ui-alert-error">Failed to load revision.</p>
      </Show>
      <Show when={revision()}>
        {(entry) => (
          <div class="settingsMain surface">
            <div class="contextBar">
              <div class="contextLeft">
                <span>
                  <b>{entryId()}</b>
                  <small>Revision {revisionId()}</small>
                </span>
              </div>
            </div>
            <pre class="code">{entry().markdown}</pre>
          </div>
        )}
      </Show>
    </SpaceShell>
  );
}
