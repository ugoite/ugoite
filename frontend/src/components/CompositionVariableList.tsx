import { createSignal, For, Show } from "solid-js";
import { IconButton } from "~/components/IconButton";
import { RowList, RowListItem } from "~/components/RowList";
import type { CompositionVariableType } from "~/lib/composition-api";
import { compositionVariableDisplayName } from "~/lib/composition-variable-name";
import type { DraftVariable } from "~/lib/composition-draft";
import { t } from "~/lib/i18n";
import { parseCompositionVariableInput } from "~/lib/ugoite-client/protocol";

const variableTypes: CompositionVariableType[] = [
  "string",
  "boolean",
  "integer",
  "float",
  "date",
  "timestamp",
];

const displayValue = (variable: DraftVariable): string => {
  const current = variable.default;
  if (current === null || current === undefined) return "";
  if (typeof current === "string") return current;
  if (typeof current === "number" || typeof current === "boolean") {
    return String(current);
  }
  return "";
};

interface CompositionVariableListProps {
  variables: DraftVariable[];
  headingId: string;
  onAdd: (variable: DraftVariable) => void;
  onUpdate: (variable: DraftVariable) => void;
  onRemove: (variableId: string) => string | undefined;
}

/**
 * Typed variable authoring for the Composition Studio. Variable IDs and
 * types remain visible; defaults and required flags edit inline. Variables
 * bind by reference; type compatibility stays a Rust-owned resolve
 * diagnostic and is never decided here.
 */
export function CompositionVariableList(props: CompositionVariableListProps) {
  const nextVariableId = (variables: readonly DraftVariable[]) => {
    let sequence = 1;
    while (variables.some((variable) => variable.id === `variable_${sequence}`)) {
      sequence += 1;
    }
    return `variable_${sequence}`;
  };
  const [newId, setNewId] = createSignal(nextVariableId(props.variables));
  const [newType, setNewType] = createSignal<CompositionVariableType>(
    "string",
  );
  const [removeError, setRemoveError] = createSignal<string | null>(null);
  const [defaultInputValues, setDefaultInputValues] = createSignal<
    Record<string, string>
  >({});
  const defaultInputGenerations = new Map<string, number>();

  const clearDefaultInputValue = (variableId: string) => {
    setDefaultInputValues((current) => {
      if (!(variableId in current)) return current;
      const next = { ...current };
      delete next[variableId];
      return next;
    });
  };

  const invalidateDefaultInput = (variableId: string) => {
    defaultInputGenerations.set(
      variableId,
      (defaultInputGenerations.get(variableId) ?? 0) + 1,
    );
    clearDefaultInputValue(variableId);
  };

  const updateDefault = async (variable: DraftVariable, raw: string) => {
    const currentVariable = () =>
      props.variables.find((item) => item.id === variable.id);
    const saveDefault = (value: unknown | undefined) => {
      const current = currentVariable();
      if (!current || current.type !== variable.type) return;
      props.onUpdate({ ...current, default: value });
    };

    const generation = (defaultInputGenerations.get(variable.id) ?? 0) + 1;
    defaultInputGenerations.set(variable.id, generation);
    setDefaultInputValues((current) => ({
      ...current,
      [variable.id]: raw,
    }));
    // Mark the draft invalid immediately while the shared parser resolves.
    // This prevents a stale, previously valid default from being saved.
    saveDefault(raw === "" ? undefined : raw);

    try {
      const parsed = await parseCompositionVariableInput(variable.type, raw);
      if (defaultInputGenerations.get(variable.id) !== generation) return;
      const current = currentVariable();
      if (!current || current.type !== variable.type) return;

      if (parsed.status === "valid") {
        saveDefault(parsed.value);
        clearDefaultInputValue(variable.id);
      } else if (parsed.status === "unset") {
        saveDefault(undefined);
        clearDefaultInputValue(variable.id);
      }
    } catch {
      // Leave the raw invalid default in the draft; Rust canonicalization
      // blocks saving it if the portable parser cannot be reached.
    }
  };

  const addVariable = () => {
    const id = newId().trim();
    if (!id || props.variables.some((variable) => variable.id === id)) return;
    props.onAdd({
      id,
      type: newType(),
      required: true,
    });
    setNewId(nextVariableId([...props.variables, { id, type: newType(), required: true }]));
  };

  const duplicateNewId = () =>
    props.variables.some((variable) => variable.id === newId().trim());
  const canAddVariable = () => newId().trim().length > 0 && !duplicateNewId();

  return (
    <div class="ui-stack-sm">
      <Show
        when={props.variables.length > 0}
        fallback={
          <p class="ui-muted">{t("composition.studioEmptyVariables")}</p>
        }
      >
        <RowList
          label={t("composition.studioVariables")}
          labelledBy={props.headingId}
        >
          <For each={props.variables}>
            {(variable) => (
              <RowListItem
                main={
                  <span class="rowListName">
                    <span>
                      {compositionVariableDisplayName(
                        props.variables,
                        variable.id,
                      )}
                    </span>
                    <span class="ui-muted">{variable.type}</span>
                  </span>
                }
                actions={
                  <IconButton
                    icon="trash"
                    label={t("composition.studioRemoveVariable", {
                      name: compositionVariableDisplayName(
                        props.variables,
                        variable.id,
                      ),
                    })}
                    onClick={() => {
                      const reason = props.onRemove(variable.id);
                      if (!reason) invalidateDefaultInput(variable.id);
                      setRemoveError(reason ?? null);
                    }}
                  />
                }
              />
            )}
          </For>
        </RowList>
      </Show>
      <Show when={removeError()}>
        <p class="ui-text-danger" role="alert">{removeError()}</p>
      </Show>
      <For each={props.variables}>
        {(variable) => (
          <div class="ui-stack-sm">
            <div class="ui-field">
              <label>
                <span>{t("composition.studioVariableType")}</span>
                <select
                  class="ui-input"
                  value={variable.type}
                  aria-label={t("composition.studioVariableTypeName", {
                    name: compositionVariableDisplayName(
                      props.variables,
                      variable.id,
                    ),
                  })}
                  onChange={(event) =>
                    (() => {
                      invalidateDefaultInput(variable.id);
                      props.onUpdate({
                        ...variable,
                        type: event.currentTarget
                          .value as CompositionVariableType,
                        default: undefined,
                      });
                    })()}
                >
                  <For each={variableTypes}>
                    {(type) => <option value={type}>{type}</option>}
                  </For>
                </select>
              </label>
            </div>
            <div class="ui-field">
              <label>
                <span>{t("composition.studioVariableDefault")}</span>
                <Show
                  when={variable.type === "boolean"}
                  fallback={
                    <input
                      class="ui-input"
                      type="text"
                      value={defaultInputValues()[variable.id] ??
                        displayValue(variable)}
                      aria-label={t("composition.studioVariableDefaultName", {
                        name: compositionVariableDisplayName(
                          props.variables,
                          variable.id,
                        ),
                      })}
                      onInput={(event) =>
                        void updateDefault(
                          variable,
                          event.currentTarget.value,
                        )}
                    />
                  }
                >
                  <select
                    class="ui-input"
                    value={displayValue(variable)}
                    aria-label={t("composition.studioVariableDefaultName", {
                      name: compositionVariableDisplayName(
                        props.variables,
                        variable.id,
                      ),
                    })}
                    onChange={(event) =>
                      props.onUpdate({
                        ...variable,
                        default: event.currentTarget.value === ""
                          ? undefined
                          : event.currentTarget.value === "true",
                      })}
                  >
                    <option value="">—</option>
                    <option value="true">{t("composition.booleanTrue")}</option>
                    <option value="false">
                      {t("composition.booleanFalse")}
                    </option>
                  </select>
                </Show>
              </label>
            </div>
            <div class="ui-field">
              <label>
                <input
                  type="checkbox"
                  checked={variable.required}
                  aria-label={t("composition.studioVariableRequiredName", {
                    name: compositionVariableDisplayName(
                      props.variables,
                      variable.id,
                    ),
                  })}
                  onChange={(event) =>
                    props.onUpdate({
                      ...variable,
                      required: event.currentTarget.checked,
                    })}
                />
                <span>{t("composition.studioVariableRequired")}</span>
              </label>
            </div>
          </div>
        )}
      </For>
      <div class="flex flex-wrap items-center gap-2">
        <label class="ui-field">
          <span class="ui-label">{t("composition.studioVariableId")}</span>
          <input
            class="ui-input"
            value={newId()}
            aria-label={t("composition.studioNewVariableId")}
            aria-invalid={!canAddVariable()}
            onInput={(event) => setNewId(event.currentTarget.value)}
          />
        </label>
        <select
          class="ui-input"
          aria-label={t("composition.studioVariableType")}
          value={newType()}
          onChange={(event) =>
            setNewType(event.currentTarget.value as CompositionVariableType)}
        >
          <For each={variableTypes}>
            {(type) => <option value={type}>{type}</option>}
          </For>
        </select>
        <button
          class="ui-button ui-button-secondary"
          type="button"
          disabled={!canAddVariable()}
          aria-label={t("composition.studioAddVariable")}
          title={t("composition.studioAddVariable")}
          onClick={addVariable}
        >
          {t("composition.studioAddVariable")}
        </button>
      </div>
    </div>
  );
}
