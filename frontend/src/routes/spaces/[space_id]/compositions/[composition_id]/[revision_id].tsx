import { createEffect, createResource, For, onCleanup, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import {
  CompositionDiagnostics,
  CompositionRenderer,
} from "~/components/CompositionRenderer";
import { FieldStack, FieldStackRow } from "~/components/FieldStack";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { t } from "~/lib/i18n";
import {
  compositionDisplayName,
  type CompositionParameterDefinition,
} from "~/lib/composition-api";
import { createCompositionQueryHandle } from "~/lib/composition-query-handle";
import { formApi } from "~/lib/ugoite-client";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "home" });

const parameterValue = (
  definition: CompositionParameterDefinition,
  value: unknown,
): string => {
  const current = value === undefined ? definition.default : value;
  if (current === null || current === undefined) return "";
  if (typeof current === "string") return current;
  if (typeof current === "number" || typeof current === "boolean") {
    return String(current);
  }
  return "";
};

const serializeParameter = (
  definition: CompositionParameterDefinition,
  value: string,
): unknown | undefined => {
  if (value === "") return undefined;
  if (definition.type === "boolean") return value === "true";
  if (definition.type === "integer" || definition.type === "float") {
    const numeric = Number(value);
    return Number.isFinite(numeric) ? numeric : value;
  }
  return value;
};

export default function CompositionRevisionRoute() {
  const params = useParams<{
    space_id: string;
    composition_id: string;
    revision_id: string;
  }>();
  const handle = createCompositionQueryHandle();

  createEffect(() => {
    void handle.open({
      spaceId: params.space_id,
      compositionId: params.composition_id,
      revisionId: params.revision_id,
    });
  });
  onCleanup(handle.dispose);

  const current = () => handle.state();
  const composition = () => current().composition;
  const name = () =>
    compositionDisplayName(
      composition()?.fields.name ?? current().summary?.name,
    );
  const definitions = () => current().resolved?.parameter_definitions ?? [];
  const diagnostics = () => current().resolved?.diagnostics ?? [];
  // Authorized Form metadata for Entry display names only. Property field
  // IDs in entry_query projections resolve to names through this existing
  // read; query semantics never depend on it and missing metadata falls
  // back to the current row key order.
  const [forms] = createResource(
    () => params.space_id,
    (spaceId) => formApi.list(spaceId).catch(() => []),
  );
  const fieldNames = (formId: string, fieldId: number): string | undefined =>
    Object.entries(
      (forms() ?? []).find((form) => form.id === formId || form.name === formId)
        ?.fields ?? {},
    ).find(([, field]) => field.id === fieldId)?.[0];
  const parameterMismatch = (parameterId: string) =>
    diagnostics().some((diagnostic) =>
      diagnostic.parameter_id === parameterId &&
      (diagnostic.code === "parameter_type_mismatch" ||
        diagnostic.code === "parameter_missing")
    );

  return (
    <>
      <h1>{name()}</h1>
      <Show when={current().opening}>
        <LocalBusyIndicator label={t("composition.detailLoading")} />
      </Show>
      <Show when={current().resolveError}>
        <section class="section" role="alert">
          <p class="ui-text-danger">{t("composition.detailFailed")}</p>
          <button
            class="ui-button ui-button-secondary"
            type="button"
            onClick={() => {
              const identity = current().identity;
              if (identity) void handle.open(identity, current().summary);
            }}
          >
            {t("composition.retry")}
          </button>
        </section>
      </Show>
      <Show when={definitions().length > 0}>
        <section class="section">
          <h2>{t("composition.parameters")}</h2>
          <FieldStack label={t("composition.parameters")}>
            <For each={definitions()}>
              {(definition) => {
                const label = () => definition.label || definition.id;
                const value = () =>
                  parameterValue(
                    definition,
                    handle.parameters()[definition.id],
                  );
                return (
                  <FieldStackRow>
                    <div class="ui-field">
                      <label>
                        <span>{label()}</span>
                        <Show
                          when={definition.type === "boolean"}
                          fallback={
                            <input
                              class="ui-input"
                              type={definition.type === "date"
                                ? "date"
                                : definition.type === "timestamp"
                                ? "datetime-local"
                                : definition.type === "integer" ||
                                    definition.type === "float"
                                ? "number"
                                : "text"}
                              step={definition.type === "integer"
                                ? "1"
                                : definition.type === "float"
                                ? "any"
                                : undefined}
                              required={definition.required || undefined}
                              value={value()}
                              aria-invalid={parameterMismatch(definition.id) ||
                                undefined}
                              onChange={(event) =>
                                handle.setParameter(
                                  definition.id,
                                  serializeParameter(
                                    definition,
                                    event.currentTarget.value,
                                  ),
                                )}
                            />
                          }
                        >
                          <select
                            class="ui-input"
                            required={definition.required || undefined}
                            value={value()}
                            aria-invalid={parameterMismatch(definition.id) ||
                              undefined}
                            onChange={(event) =>
                              handle.setParameter(
                                definition.id,
                                serializeParameter(
                                  definition,
                                  event.currentTarget.value,
                                ),
                              )}
                          >
                            <option value="">—</option>
                            <option value="true">
                              {t("composition.booleanTrue")}
                            </option>
                            <option value="false">
                              {t("composition.booleanFalse")}
                            </option>
                          </select>
                        </Show>
                      </label>
                    </div>
                  </FieldStackRow>
                );
              }}
            </For>
          </FieldStack>
        </section>
      </Show>
      <Show when={diagnostics().length > 0}>
        <CompositionDiagnostics
          diagnostics={diagnostics()}
          parameterDefinitions={definitions()}
        />
      </Show>
      <Show when={current().resolving}>
        <LocalBusyIndicator label={t("composition.queryLoading")} />
      </Show>
      <Show when={current().resolved?.ok && current().resolved?.plan}>
        {(plan) => (
          <CompositionRenderer
            plan={plan()}
            sources={current().sources}
            fieldNames={fieldNames}
            onNext={handle.next}
            onPrevious={handle.previous}
            onRetry={handle.retry}
          />
        )}
      </Show>
    </>
  );
}
