import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createSignal, For, Show } from "solid-js";
import { sqlApi, sqlSessionApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";

export default function SpaceQueryVariablesRoute() {
  const params = useParams<{ space_id: string; query_id: string }>();
  const spaceId = () => params.space_id;
  const queryId = () => params.query_id;
  const navigate = useNavigate();
  const [values, setValues] = createSignal<Record<string, string>>({});
  const [error, setError] = createSignal<string | null>(null);

  const [entry] = createResource(async () => sqlApi.get(spaceId(), queryId()));

  const variables = createMemo(() => entry()?.variables || []);

  const handleInputChange = (name: string, value: string) => {
    setValues((prev) => ({ ...prev, [name]: value }));
  };

  const handleRun = async () => {
    setError(null);
    const current = entry();
    if (!current) return;
    try {
      const parameterTypes = Object.fromEntries(
        current.variables.map((variable) => [variable.name, variable.type]),
      );
      const parameters = Object.fromEntries(
        current.variables.map((variable) => {
          const value = values()[variable.name] ?? "";
          if (value === "") return [variable.name, null];
          if (variable.type === "boolean") {
            if (value !== "true" && value !== "false") {
              throw new Error(`${variable.name} must be true or false`);
            }
            return [variable.name, value === "true"];
          }
          if (variable.type === "integer" || variable.type === "float") {
            const numeric = Number(value);
            if (!Number.isFinite(numeric)) {
              throw new Error(`${variable.name} must be a number`);
            }
            return [variable.name, numeric];
          }
          return [variable.name, value];
        }),
      );
      const session = await sqlSessionApi.create(
        spaceId(),
        current.sql,
        parameters,
        parameterTypes,
      );
      navigate(
        `/spaces/${spaceId()}/entries?session=${
          encodeURIComponent(session.id)
        }`,
      );
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to run query");
    }
  };

  return (
    <>
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
    </>
  );
}
