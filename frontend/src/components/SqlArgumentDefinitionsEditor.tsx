import { createSignal, For, Show } from "solid-js";
import { t } from "~/lib/i18n";
import { parseSqlVariableDefault, sqlVariableDefaultDraft } from "~/lib/sql";
import type { SqlVariableDefaultDraft } from "~/lib/sql";
import type { SqlVariable } from "~/lib/types";
import type { SqlStructureVariableType } from "~/lib/ugoite-client";
import { IconButton } from "./IconButton";

const variableTypes: readonly SqlStructureVariableType[] = [
  "string",
  "boolean",
  "integer",
  "float",
  "date",
  "timestamp",
];

interface Props {
  idPrefix: string;
  variables: readonly SqlVariable[];
  referencedNames: readonly string[];
  defaults: Record<string, SqlVariableDefaultDraft>;
  disabled?: boolean;
  allowDefaults?: boolean;
  onAdd: (variable: SqlVariable) => boolean | void;
  onUpdate: (
    oldName: string,
    variable: SqlVariable,
  ) => boolean | void | Promise<boolean | void>;
  onRemove: (name: string) => boolean | void | Promise<boolean | void>;
  onDefaultsChange: (defaults: Record<string, SqlVariableDefaultDraft>) => void;
}

export function SqlArgumentDefinitionsEditor(props: Props) {
  const [newName, setNewName] = createSignal("");
  const [newType, setNewType] = createSignal<SqlStructureVariableType>(
    "string",
  );
  const [renameDrafts, setRenameDrafts] = createSignal<Record<string, string>>(
    {},
  );
  const [pendingName, setPendingName] = createSignal<string>();
  const [error, setError] = createSignal<string>();
  let editorRoot: HTMLDivElement | undefined;

  // Host updates can replace the edited row. Keep keyboard focus in this
  // dialog, unless the user has deliberately focused another control.
  const restoreEditedFocus = (id: string, ownedFocus: boolean) => {
    queueMicrotask(() => {
      if (
        !ownedFocus || !editorRoot?.isConnected ||
        document.activeElement !== document.body
      ) return;
      const target = document.getElementById(id);
      if (target && editorRoot.contains(target)) target.focus();
      if (document.activeElement === document.body) {
        editorRoot.closest<HTMLElement>('[role="dialog"]')?.focus();
      }
    });
  };

  const draftName = (variable: SqlVariable) =>
    renameDrafts()[variable.name] ?? variable.name;
  const draftFor = (variable: SqlVariable) =>
    props.defaults[variable.name] ?? sqlVariableDefaultDraft(variable);
  const usedNames = () => new Set(props.referencedNames);
  const nameTaken = (name: string, except?: string) =>
    props.variables.some((variable) =>
      variable.name === name && variable.name !== except
    );
  const validName = (name: string) =>
    /^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(name);
  const newNameError = () => {
    if (!newName()) return undefined;
    if (!validName(newName())) {
      return t("sqlPage.structure.argumentNameInvalid");
    }
    if (nameTaken(newName())) return t("sqlPage.structure.argumentNameTaken");
    return undefined;
  };

  const updateDraft = (
    variable: SqlVariable,
    change: Partial<SqlVariableDefaultDraft>,
  ) => {
    props.onDefaultsChange({
      ...props.defaults,
      [variable.name]: { ...draftFor(variable), ...change },
    });
  };

  const addArgument = () => {
    if (!validName(newName()) || nameTaken(newName()) || props.disabled) return;
    const variable: SqlVariable = {
      name: newName(),
      type: newType(),
      description: "",
    };
    if (props.onAdd(variable) === false) {
      setError(t("sqlPage.structureUnavailable"));
      return;
    }
    setNewName("");
    setError(undefined);
  };

  const renameArgument = async (variable: SqlVariable) => {
    const nextName = draftName(variable);
    if (
      props.disabled || !validName(nextName) || nextName === variable.name ||
      nameTaken(nextName, variable.name) || pendingName()
    ) return;
    const ownedFocus = editorRoot?.contains(document.activeElement) ?? false;
    let focusName = variable.name;
    setPendingName(variable.name);
    setError(undefined);
    try {
      if (
        await props.onUpdate(variable.name, { ...variable, name: nextName }) ===
          false
      ) {
        setError(t("sqlPage.structureUnavailable"));
        return;
      }
      setRenameDrafts((current) => {
        const next = { ...current };
        delete next[variable.name];
        return next;
      });
      focusName = nextName;
    } catch {
      setError(t("sqlPage.structureUnavailable"));
    } finally {
      setPendingName(undefined);
      restoreEditedFocus(`${props.idPrefix}-${focusName}-rename`, ownedFocus);
    }
  };

  const changeType = async (variable: SqlVariable, type: string) => {
    if (
      props.disabled || pendingName() ||
      !variableTypes.includes(type as SqlStructureVariableType)
    ) return;
    const ownedFocus = editorRoot?.contains(document.activeElement) ?? false;
    setPendingName(variable.name);
    setError(undefined);
    try {
      if (
        await props.onUpdate(variable.name, { ...variable, type }) === false
      ) setError(t("sqlPage.structureUnavailable"));
    } catch {
      setError(t("sqlPage.structureUnavailable"));
    } finally {
      setPendingName(undefined);
      restoreEditedFocus(`${props.idPrefix}-${variable.name}-type`, ownedFocus);
    }
  };

  const removeArgument = async (variable: SqlVariable) => {
    if (props.disabled || pendingName() || usedNames().has(variable.name)) {
      return;
    }
    const ownedFocus = editorRoot?.contains(document.activeElement) ?? false;
    setPendingName(variable.name);
    setError(undefined);
    try {
      if (await props.onRemove(variable.name) === false) {
        setError(t("sqlPage.structureUnavailable"));
        return;
      }
      props.onDefaultsChange(
        Object.fromEntries(
          Object.entries(props.defaults).filter(([name]) =>
            name !== variable.name
          ),
        ),
      );
    } catch {
      setError(t("sqlPage.structureUnavailable"));
    } finally {
      setPendingName(undefined);
      restoreEditedFocus(`${props.idPrefix}-argument-name`, ownedFocus);
    }
  };

  const argumentDefaultInvalid = (variable: SqlVariable) => {
    const draft = draftFor(variable);
    return draft.enabled &&
      !parseSqlVariableDefault(variable.type, draft.raw).valid;
  };

  return (
    <div
      class="sql-structure-arguments"
      ref={(element) => editorRoot = element}
    >
      <div class="sql-structure-argument-list" role="list">
        <For each={props.variables}>
          {(variable) => {
            const renameId = `${props.idPrefix}-${variable.name}-rename`;
            const typeId = `${props.idPrefix}-${variable.name}-type`;
            const defaultId = `${props.idPrefix}-${variable.name}-default`;
            const checkboxId = `${defaultId}-enabled`;
            const draft = () => draftFor(variable);
            const parsed = () =>
              parseSqlVariableDefault(variable.type, draft().raw);
            const inputLabel = t("sqlPage.defaultFor", {
              name: variable.name,
            });
            const isUsed = () => usedNames().has(variable.name);
            const renameError = () => {
              const nextName = draftName(variable);
              if (nextName === variable.name) return undefined;
              if (!validName(nextName)) {
                return t("sqlPage.structure.argumentNameInvalid");
              }
              if (nameTaken(nextName, variable.name)) {
                return t("sqlPage.structure.argumentNameTaken");
              }
              return undefined;
            };
            const typeOptions = () =>
              variableTypes.includes(variable.type as SqlStructureVariableType)
                ? variableTypes
                : [variable.type, ...variableTypes];

            return (
              <div class="sql-structure-argument" role="listitem">
                <div class="sql-structure-argument-controls">
                  <label class="ui-field" for={renameId}>
                    <span>
                      {t("sqlPage.structure.argumentNameFor", {
                        name: variable.name,
                      })}
                    </span>
                    <input
                      id={renameId}
                      class="ui-input"
                      type="text"
                      autoComplete="off"
                      spellcheck={false}
                      value={draftName(variable)}
                      aria-invalid={Boolean(renameError()) || undefined}
                      disabled={props.disabled || pendingName() !== undefined ||
                        pendingName() === variable.name}
                      onInput={(event) =>
                        setRenameDrafts((current) => ({
                          ...current,
                          [variable.name]: event.currentTarget.value,
                        }))}
                    />
                  </label>
                  <Show when={draftName(variable) !== variable.name}>
                    <IconButton
                      icon="edit"
                      label={t("sqlPage.structure.renameArgument")}
                      disabled={Boolean(renameError()) || props.disabled ||
                        pendingName() !== undefined}
                      onClick={() => void renameArgument(variable)}
                    />
                  </Show>
                  <label class="ui-field" for={typeId}>
                    <span>
                      {t("sqlPage.structure.argumentTypeFor", {
                        name: variable.name,
                      })}
                    </span>
                    <select
                      id={typeId}
                      class="ui-input"
                      value={variable.type}
                      disabled={props.disabled || pendingName() !== undefined ||
                        !variableTypes.includes(
                          variable.type as SqlStructureVariableType,
                        )}
                      onChange={(event) =>
                        changeType(variable, event.currentTarget.value)}
                    >
                      <For each={typeOptions()}>
                        {(type) => <option value={type}>{type}</option>}
                      </For>
                    </select>
                  </label>
                  <Show when={props.allowDefaults !== false}>
                    <label
                      class="flex min-h-11 items-center gap-2"
                      for={checkboxId}
                    >
                      <input
                        id={checkboxId}
                        type="checkbox"
                        aria-label={inputLabel}
                        checked={draft().enabled}
                        disabled={props.disabled ||
                          pendingName() !== undefined ||
                          !variableTypes.includes(
                            variable.type as SqlStructureVariableType,
                          )}
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
                          id={defaultId}
                          class="ui-input"
                          type="text"
                          inputMode={variable.type === "integer"
                            ? "numeric"
                            : variable.type === "float"
                            ? "decimal"
                            : "text"}
                          aria-label={inputLabel}
                          aria-invalid={draft().enabled && !parsed().valid ||
                            undefined}
                          disabled={props.disabled ||
                            pendingName() !== undefined || !draft().enabled ||
                            !variableTypes.includes(
                              variable.type as SqlStructureVariableType,
                            )}
                          value={draft().raw}
                          onInput={(event) =>
                            updateDraft(variable, {
                              raw: event.currentTarget.value,
                            })}
                        />
                      }
                    >
                      <select
                        id={defaultId}
                        class="ui-input"
                        aria-label={inputLabel}
                        aria-invalid={draft().enabled && !parsed().valid ||
                          undefined}
                        disabled={props.disabled ||
                          pendingName() !== undefined || !draft().enabled}
                        value={draft().raw}
                        onChange={(event) =>
                          updateDraft(variable, {
                            raw: event.currentTarget.value,
                          })}
                      >
                        <option value=""></option>
                        <option value="true">true</option>
                        <option value="false">false</option>
                      </select>
                    </Show>
                  </Show>
                  <IconButton
                    icon="trash"
                    label={t("sqlPage.structure.removeArgument", {
                      name: variable.name,
                    })}
                    disabled={props.disabled || pendingName() !== undefined ||
                      isUsed()}
                    title={isUsed()
                      ? t("sqlPage.structure.argumentRemoveUsed")
                      : undefined}
                    onClick={() => removeArgument(variable)}
                  />
                </div>
                <Show when={!isUsed()}>
                  <span class="ui-text-danger" role="alert">
                    {t("sqlPage.structure.argumentUnused")}
                  </span>
                </Show>
                <Show when={renameError()}>
                  <span class="ui-text-danger" role="alert">
                    {renameError()}
                  </span>
                </Show>
                <Show
                  when={draft().enabled && argumentDefaultInvalid(variable)}
                >
                  <span class="ui-text-danger" role="alert">
                    {t("sqlPage.defaultInvalid", {
                      name: variable.name,
                      type: variable.type,
                    })}
                  </span>
                </Show>
              </div>
            );
          }}
        </For>
      </div>
      <div class="sql-structure-argument-add">
        <label class="ui-field" for={`${props.idPrefix}-argument-name`}>
          <span>{t("sqlPage.structure.argumentName")}</span>
          <input
            id={`${props.idPrefix}-argument-name`}
            class="ui-input"
            type="text"
            autoComplete="off"
            spellcheck={false}
            value={newName()}
            aria-invalid={Boolean(newNameError()) || undefined}
            disabled={props.disabled || pendingName() !== undefined}
            onInput={(event) => setNewName(event.currentTarget.value)}
          />
        </label>
        <label class="ui-field" for={`${props.idPrefix}-argument-type`}>
          <span>{t("sqlPage.structure.argumentType")}</span>
          <select
            id={`${props.idPrefix}-argument-type`}
            class="ui-input"
            value={newType()}
            disabled={props.disabled || pendingName() !== undefined}
            onChange={(event) =>
              setNewType(event.currentTarget.value as SqlStructureVariableType)}
          >
            <For each={variableTypes}>
              {(type) => <option value={type}>{type}</option>}
            </For>
          </select>
        </label>
        <button
          type="button"
          class="ui-button ui-button-primary"
          onClick={addArgument}
          disabled={props.disabled || pendingName() !== undefined ||
            !validName(newName()) ||
            Boolean(newNameError())}
        >
          {t("sqlPage.structure.addArgument")}
        </button>
      </div>
      <Show when={error()}>
        <span class="ui-text-danger" role="alert">{error()}</span>
      </Show>
    </div>
  );
}
