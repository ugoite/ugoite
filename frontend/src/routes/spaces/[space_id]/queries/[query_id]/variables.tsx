import { A, useParams } from "@solidjs/router";
import { createMemo, createSignal, For, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { sqlApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";

export default function SpaceQueryVariablesRoute() {
  const params = useParams<{ space_id: string; query_id: string }>();
  const spaceId = () => params.space_id;
  const queryId = () => params.query_id;
  const [values, setValues] = createSignal<Record<string, string>>({});
  const [error, setError] = createSignal<string | null>(null);

  const [entry] = createResource(async () => sqlApi.get(spaceId(), queryId()));

  const variables = createMemo(() => entry()?.variables || []);

  const handleInputChange = (name: string, value: string) => {
    setValues((prev) => ({ ...prev, [name]: value }));
  };

  const handleRun = async () => {
    setError(null);
    setError(
      "Template parameters are not supported by the DataFusion SQL surface yet.",
    );
  };

  return (
    <SpaceShell
      spaceId={spaceId()}
      activeNavigation="search"
      title="SQL / Variables"
    >
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">Search / Saved SQL</div>
          <h1>Query variables</h1>
        </div>
      </div>

      <Show when={entry.loading}>
        <p class="text-sm ui-muted mt-4">Loading query...</p>
      </Show>
      <Show when={entry.error}>
        <p class="text-sm ui-text-danger mt-4">Failed to load query.</p>
      </Show>
      <Show when={entry()}>
        {(data) => (
          <div class="settingsMain surface">
            <p class="text-sm ui-muted">{data().name}</p>
            <div class="ui-stack-sm">
              <For each={variables()}>
                {(variable, index) => {
                  const inputId = `query-var-${variable.name}-${index()}`;
                  return (
                    <div>
                      <label class="ui-label" for={inputId}>
                        {variable.name}
                        <span class="ml-2 text-xs ui-muted">
                          {variable.type}
                        </span>
                      </label>
                      <input
                        id={inputId}
                        class="ui-input"
                        placeholder={variable.description}
                        value={values()[variable.name] ?? ""}
                        onInput={(e) =>
                          handleInputChange(
                            variable.name,
                            e.currentTarget.value,
                          )}
                      />
                    </div>
                  );
                }}
              </For>
            </div>
            <Show when={error()}>
              <p class="text-sm ui-text-danger">{error()}</p>
            </Show>
            <button
              type="button"
              class="btn primary"
              onClick={handleRun}
            >
              Run
            </button>
          </div>
        )}
      </Show>
    </SpaceShell>
  );
}
