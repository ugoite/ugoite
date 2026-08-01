import { A, useNavigate, useParams } from "@solidjs/router";
import { createSignal, For, Show } from "solid-js";
import { entryApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";

export default function SpaceEntryRestoreRoute() {
  const navigate = useNavigate();
  const params = useParams<{ space_id: string; entry_id: string }>();
  const spaceId = () => params.space_id;
  const entryId = () => params.entry_id;
  const encodedEntryId = () => encodeURIComponent(entryId());
  const [selectedRevision, setSelectedRevision] = createSignal<string | null>(
    null,
  );
  const [restoreError, setRestoreError] = createSignal<string | null>(null);
  const [isRestoring, setIsRestoring] = createSignal(false);

  const [history] = createResource(async () => {
    return await entryApi.history(spaceId(), entryId());
  });

  const handleRestore = async () => {
    const revisionId = selectedRevision();
    if (!revisionId) return;
    setIsRestoring(true);
    setRestoreError(null);
    try {
      await entryApi.restore(spaceId(), entryId(), revisionId);
      navigate(`/spaces/${spaceId()}/entries/${encodedEntryId()}`);
    } catch (err) {
      setRestoreError(
        err instanceof Error ? err.message : "Failed to restore entry",
      );
    } finally {
      setIsRestoring(false);
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{entryId()} / History</div>
          <h1>Restore Entry</h1>
        </div>
        <A
          href={`/spaces/${spaceId()}/entries/${encodedEntryId()}`}
          class="btn"
        >
          Back to Entry
        </A>
      </div>

      <Show when={history.loading}>
        <p class="ui-muted">Loading revisions...</p>
      </Show>
      <Show when={history.error}>
        <p class="ui-alert ui-alert-error">Failed to load history.</p>
      </Show>
      <Show when={history()}>
        {(data) => (
          <div class="settingsMain surface">
            <p class="ui-muted">Select a revision to restore.</p>
            <ul class="rowStack">
              <For each={data().revisions}>
                {(revision) => (
                  <li class="rowBtn">
                    <input
                      type="radio"
                      name="revision"
                      value={revision.revision_id}
                      checked={selectedRevision() === revision.revision_id}
                      onChange={() => setSelectedRevision(revision.revision_id)}
                    />
                    <span>
                      <b>{revision.revision_id}</b>
                      <small>{revision.created_at}</small>
                    </span>
                  </li>
                )}
              </For>
            </ul>
            <button
              type="button"
              class="btn primary"
              onClick={handleRestore}
              disabled={!selectedRevision() || isRestoring()}
            >
              {isRestoring() ? "Restoring..." : "Restore Selected Revision"}
            </button>
            <Show when={restoreError()}>
              <p class="ui-alert ui-alert-error">{restoreError()}</p>
            </Show>
          </div>
        )}
      </Show>
    </>
  );
}
