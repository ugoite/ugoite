import { createSignal, For, Show } from "solid-js";
import { IconButton } from "~/components/IconButton";
import { RowList, RowListItem } from "~/components/RowList";
import type { CompositionParameterType } from "~/lib/composition-api";
import type { DraftParameter } from "~/lib/composition-draft";
import { t } from "~/lib/i18n";

const parameterTypes: CompositionParameterType[] = [
  "string",
  "boolean",
  "integer",
  "float",
  "date",
  "timestamp",
];

const displayValue = (parameter: DraftParameter): string => {
  const current = parameter.default;
  if (current === null || current === undefined) return "";
  if (typeof current === "string") return current;
  if (typeof current === "number" || typeof current === "boolean") {
    return String(current);
  }
  return "";
};

const parseValue = (
  type: CompositionParameterType,
  value: string,
): unknown | undefined => {
  if (value === "") return undefined;
  if (type === "boolean") return value === "true";
  if (type === "integer" || type === "float") {
    const numeric = Number(value);
    return Number.isFinite(numeric) ? numeric : value;
  }
  return value;
};

const parameterDisplayName = (parameter: DraftParameter): string =>
  parameter.label?.trim() || parameter.id;

interface CompositionParameterListProps {
  parameters: DraftParameter[];
  headingId: string;
  onAdd: (parameter: DraftParameter) => void;
  onUpdate: (parameter: DraftParameter) => void;
  onRemove: (parameterId: string) => string | undefined;
}

/**
 * Typed parameter authoring for the Composition Studio. Rows carry identity
 * (id + type); labels, defaults, and required flags edit inline. Parameters
 * bind by reference; type compatibility stays a Rust-owned resolve
 * diagnostic and is never decided here.
 */
export function CompositionParameterList(props: CompositionParameterListProps) {
  const [newId, setNewId] = createSignal("");
  const [newType, setNewType] = createSignal<CompositionParameterType>(
    "string",
  );
  const [removeError, setRemoveError] = createSignal<string | null>(null);

  const addParameter = () => {
    const id = newId().trim();
    if (!id) return;
    props.onAdd({ id, type: newType(), required: true });
    setNewId("");
  };

  const idTaken = () =>
    props.parameters.some((item) => item.id === newId().trim());

  // A disabled Add carries its reason in the accessible name and title,
  // mirroring the Studio save gate. Both reasons reuse existing vocabulary
  // with no new copy: an empty field needs a parameter name, and a taken
  // field names the colliding id.
  const addBlockedReason = (): string | undefined => {
    if (newId().trim().length === 0) return t("composition.studioParameterId");
    if (idTaken()) return newId().trim();
    return undefined;
  };

  const addLabel = (): string => {
    const reason = addBlockedReason();
    return reason
      ? `${t("composition.studioAddParameter")}: ${reason}`
      : t("composition.studioAddParameter");
  };

  return (
    <div class="ui-stack-sm">
      <Show
        when={props.parameters.length > 0}
        fallback={
          <p class="ui-muted">{t("composition.studioEmptyParameters")}</p>
        }
      >
        <RowList
          label={t("composition.studioParameters")}
          labelledBy={props.headingId}
        >
          <For each={props.parameters}>
            {(parameter) => (
              <RowListItem
                main={
                  <span class="rowListName">
                    <span>{parameterDisplayName(parameter)}</span>
                    <span class="ui-muted">{parameter.type}</span>
                  </span>
                }
                actions={
                  <IconButton
                    icon="trash"
                    label={t("composition.studioRemoveParameter", {
                      name: parameterDisplayName(parameter),
                    })}
                    onClick={() => {
                      const reason = props.onRemove(parameter.id);
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
      <For each={props.parameters}>
        {(parameter) => (
          <div class="ui-stack-sm">
            <div class="ui-field">
              <label>
                <span>{t("composition.studioParameterLabel")}</span>
                <input
                  class="ui-input"
                  value={parameter.label ?? ""}
                  aria-label={t("composition.studioParameterLabelName", {
                    name: parameterDisplayName(parameter),
                  })}
                  onChange={(event) =>
                    props.onUpdate({
                      ...parameter,
                      label: event.currentTarget.value.trim() || undefined,
                    })}
                />
              </label>
            </div>
            <div class="ui-field">
              <label>
                <span>{t("composition.studioParameterDefault")}</span>
                <Show
                  when={parameter.type === "boolean"}
                  fallback={
                    <input
                      class="ui-input"
                      type={parameter.type === "date"
                        ? "date"
                        : parameter.type === "timestamp"
                        ? "datetime-local"
                        : parameter.type === "integer" ||
                            parameter.type === "float"
                        ? "number"
                        : "text"}
                      step={parameter.type === "integer" ? "1" : undefined}
                      value={displayValue(parameter)}
                      aria-label={t("composition.studioParameterDefaultName", {
                        name: parameterDisplayName(parameter),
                      })}
                      onChange={(event) =>
                        props.onUpdate({
                          ...parameter,
                          default: parseValue(
                            parameter.type,
                            event.currentTarget.value,
                          ),
                        })}
                    />
                  }
                >
                  <select
                    class="ui-input"
                    value={displayValue(parameter)}
                    aria-label={t("composition.studioParameterDefaultName", {
                      name: parameterDisplayName(parameter),
                    })}
                    onChange={(event) =>
                      props.onUpdate({
                        ...parameter,
                        default: parseValue(
                          parameter.type,
                          event.currentTarget.value,
                        ),
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
                  checked={parameter.required}
                  aria-label={t("composition.studioParameterRequiredName", {
                    name: parameterDisplayName(parameter),
                  })}
                  onChange={(event) =>
                    props.onUpdate({
                      ...parameter,
                      required: event.currentTarget.checked,
                    })}
                />
                <span>{t("composition.studioParameterRequired")}</span>
              </label>
            </div>
          </div>
        )}
      </For>
      <div class="flex flex-wrap items-center gap-2">
        <input
          class="ui-input"
          aria-label={t("composition.studioParameterId")}
          value={newId()}
          onInput={(event) => setNewId(event.currentTarget.value)}
        />
        <select
          class="ui-input"
          aria-label={t("composition.studioParameterType")}
          value={newType()}
          onChange={(event) =>
            setNewType(event.currentTarget.value as CompositionParameterType)}
        >
          <For each={parameterTypes}>
            {(type) => <option value={type}>{type}</option>}
          </For>
        </select>
        <button
          class="ui-button ui-button-secondary"
          type="button"
          disabled={addBlockedReason() !== undefined}
          aria-label={addLabel()}
          title={addLabel()}
          onClick={addParameter}
        >
          {t("composition.studioAddParameter")}
        </button>
      </div>
    </div>
  );
}
