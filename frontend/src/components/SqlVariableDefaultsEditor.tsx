import { For, Show } from "solid-js";
import {
  parseSqlVariableDefault,
  type SqlVariableDefaultDraft,
  sqlVariableDefaultDraft,
} from "~/lib/sql";
import { t } from "~/lib/i18n";
import type { SqlVariable } from "~/lib/types";

interface Props {
  idPrefix: string;
  variables: SqlVariable[];
  defaults: Record<string, SqlVariableDefaultDraft>;
  disabled?: boolean;
  onDefaultsChange: (defaults: Record<string, SqlVariableDefaultDraft>) => void;
}

export function SqlVariableDefaultsEditor(props: Props) {
  const draftFor = (variable: SqlVariable) =>
    props.defaults[variable.name] ?? sqlVariableDefaultDraft(variable);

  const updateDraft = (
    variable: SqlVariable,
    change: Partial<SqlVariableDefaultDraft>,
  ) => {
    props.onDefaultsChange({
      ...props.defaults,
      [variable.name]: { ...draftFor(variable), ...change },
    });
  };

  return (
    <div class="ui-stack-sm">
      <For each={props.variables}>
        {(variable) => {
          const checkboxId =
            `${props.idPrefix}-${variable.name}-default-enabled`;
          const draft = () => draftFor(variable);
          const parsed = () =>
            parseSqlVariableDefault(variable.type, draft().raw);
          const inputLabel = t("sqlPage.defaultFor", { name: variable.name });

          return (
            <div class="ui-stack-sm">
              <div class="flex flex-wrap items-baseline gap-x-2">
                <span class="font-medium">{variable.name}</span>
                <span class="text-xs ui-muted">{variable.type}</span>
                <Show when={variable.description}>
                  <span class="text-sm ui-muted">{variable.description}</span>
                </Show>
              </div>
              <div class="flex flex-wrap items-center gap-3">
                <label
                  class="flex min-h-11 items-center gap-2"
                  for={checkboxId}
                >
                  <input
                    id={checkboxId}
                    type="checkbox"
                    aria-label={inputLabel}
                    checked={draft().enabled}
                    disabled={props.disabled}
                    onChange={(event) => {
                      const enabled = event.currentTarget.checked;
                      updateDraft(variable, {
                        enabled,
                        raw: enabled && variable.type === "boolean" &&
                            draft().raw === ""
                          ? "true"
                          : draft().raw,
                      });
                    }}
                  />
                  {t("sqlPage.defaultValue")}
                </label>
                <Show
                  when={variable.type === "boolean"}
                  fallback={
                    <input
                      class="ui-input min-w-40"
                      type="text"
                      inputMode={variable.type === "integer"
                        ? "numeric"
                        : variable.type === "float"
                        ? "decimal"
                        : "text"}
                      aria-label={inputLabel}
                      aria-invalid={draft().enabled && !parsed().valid ||
                        undefined}
                      disabled={props.disabled || !draft().enabled}
                      value={draft().raw}
                      onInput={(event) =>
                        updateDraft(variable, {
                          raw: event.currentTarget.value,
                        })}
                    />
                  }
                >
                  <select
                    class="ui-input min-w-40"
                    aria-label={inputLabel}
                    aria-invalid={draft().enabled && !parsed().valid ||
                      undefined}
                    disabled={props.disabled || !draft().enabled}
                    value={draft().raw}
                    onChange={(event) =>
                      updateDraft(variable, { raw: event.currentTarget.value })}
                  >
                    <option value=""></option>
                    <option value="true">true</option>
                    <option value="false">false</option>
                  </select>
                </Show>
              </div>
              <Show when={draft().enabled && !parsed().valid}>
                <p class="text-sm ui-text-danger" role="alert">
                  {t("sqlPage.defaultInvalid", {
                    name: variable.name,
                    type: variable.type,
                  })}
                </p>
              </Show>
            </div>
          );
        }}
      </For>
    </div>
  );
}
