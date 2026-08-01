import { A, useNavigate, useParams } from "@solidjs/router";
import type { Diagnostic } from "@codemirror/lint";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { SqlQueryEditor } from "~/components";
import { formApi } from "~/lib/ugoite-client";
import { buildSqlSchema } from "~/lib/sql";
import { sqlApi } from "~/lib/ugoite-client";
import type { Form, SqlVariable } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";

const VARIABLE_REGEX = /\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}/g;

function extractVariables(sql: string): SqlVariable[] {
  const names = new Set<string>();
  VARIABLE_REGEX.lastIndex = 0;
  let match = VARIABLE_REGEX.exec(sql);
  while (match !== null) {
    names.add(match[1]);
    match = VARIABLE_REGEX.exec(sql);
  }
  return Array.from(names).map((name) => ({
    type: "string",
    name,
    description: `Variable ${name}`,
  }));
}

export default function SpaceQueryCreateRoute() {
  const params = useParams<{ space_id: string }>();
  const navigate = useNavigate();
  const spaceId = () => params.space_id;
  const [queryName, setQueryName] = createSignal("");
  const [sqlInput, setSqlInput] = createSignal("");
  const [diagnostics, setDiagnostics] = createSignal<Diagnostic[]>([]);
  const [error, setError] = createSignal<string | null>(null);
  const [isSaving, setIsSaving] = createSignal(false);

  const [forms] = createResource(async () => {
    return await formApi.list(spaceId());
  });

  const defaultSql = createMemo(() => {
    const relation = forms()?.find((form) => form.sql_relation)?.sql_relation;
    return relation
      ? `SELECT * FROM "${relation}" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 50`
      : "";
  });

  createEffect(() => {
    if (!sqlInput().trim() && defaultSql()) setSqlInput(defaultSql());
  });

  const schema = () => buildSqlSchema((forms() || []) as Form[]);

  const handleSave = async () => {
    setError(null);
    const name = queryName().trim() || "Untitled query";
    const sql = sqlInput().trim();
    if (!sql) {
      setError("SQL is required.");
      return;
    }

    setIsSaving(true);
    try {
      await sqlApi.create(spaceId(), {
        name,
        sql,
        variables: extractVariables(sql),
      });
      navigate(`/spaces/${spaceId()}/search`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save query");
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">Search / Saved SQL</div>
          <h1>New SQL</h1>
        </div>
      </div>
      <div class="settingsMain surface">
        <label class="ui-label" for="query-title">
          Query name
        </label>
        <input
          id="query-title"
          class="ui-input"
          placeholder="Untitled query"
          value={queryName()}
          onInput={(e) =>
            setQueryName(e.currentTarget.value)}
        />

        <div>
          <label class="ui-label mb-2" for="query-sql">
            SQL
          </label>
          <SqlQueryEditor
            id="query-sql"
            value={sqlInput()}
            onChange={setSqlInput}
            schema={schema()}
            onDiagnostics={setDiagnostics}
            disabled={isSaving()}
          />
        </div>

        <Show when={diagnostics().length > 0}>
          <ul class="text-sm ui-text-warning ui-stack-sm">
            <For each={diagnostics()}>
              {(diag) => <li>{diag.message}</li>}
            </For>
          </ul>
        </Show>
        <Show when={error()}>
          <p class="text-sm ui-text-danger">{error()}</p>
        </Show>

        <button
          type="button"
          class="btn primary"
          onClick={handleSave}
          disabled={isSaving()}
        >
          {isSaving() ? "Saving..." : "Save"}
        </button>
      </div>
    </>
  );
}
