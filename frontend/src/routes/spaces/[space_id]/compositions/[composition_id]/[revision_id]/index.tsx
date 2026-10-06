import {
  createEffect,
  createResource,
  createSignal,
  For,
  onCleanup,
  Show,
} from "solid-js";
import { A, useNavigate, useParams } from "@solidjs/router";
import {
  CompositionDiagnostics,
  CompositionRenderer,
} from "~/components/CompositionRenderer";
import { FieldStack, FieldStackRow } from "~/components/FieldStack";
import { IconButton } from "~/components/IconButton";
import { IconLink } from "~/components/IconLink";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { t } from "~/lib/i18n";
import {
  compositionApi,
  compositionDisplayName,
  type CompositionParameterDefinition,
} from "~/lib/composition-api";
import { createCompositionQueryHandle } from "~/lib/composition-query-handle";
import { formApi } from "~/lib/ugoite-client";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import { formatUserFacingError } from "~/lib/user-facing-error";
import {
  spaceCompositionEditPath,
  spaceCompositionHistoryPath,
  spaceCompositionRevisionPath,
} from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";
import type { Form } from "~/lib/types";

export const route = spaceRoute({ navigation: "home" });

/**
 * Display-only Form field-name lookup for Composition entry_query tables.
 * Matches the stable Form id only; display names never participate so a
 * Form rename cannot change which fields resolve. Query semantics never
 * depend on this helper — missing metadata returns undefined and the
 * table falls back to the current row key order.
 */
export const resolveCompositionFieldName = (
  forms: readonly Form[] | undefined,
  formId: string,
  fieldId: number,
): string | undefined =>
  Object.entries(
    (forms ?? []).find((form) => form.id === formId)?.fields ?? {},
  ).find(([, field]) => field.id === fieldId)?.[0];

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
  const navigate = useNavigate();
  const handle = createCompositionQueryHandle();
  const historyHref = () =>
    spaceCompositionHistoryPath(params.space_id, params.composition_id);

  // The latest revision comes from the bounded list projection. An opened
  // revision that is not the latest is historical and offers a single
  // append-only Restore action; the latest revision offers none.
  const [listing] = createResource(
    () => params.space_id,
    (spaceId) => compositionApi.list(spaceId, 100, 0).catch(() => undefined),
  );
  const latestRevisionId = () =>
    listing()?.items.find(
      (item) => item.composition_id === params.composition_id,
    )?.revision_id;
  const isHistorical = () => {
    const latest = latestRevisionId();
    return latest !== undefined && latest !== params.revision_id;
  };

  const [restoring, setRestoring] = createSignal(false);
  const [restoreError, setRestoreError] = createSignal<string | null>(null);
  const [restoreConflict, setRestoreConflict] = createSignal(false);
  const [restoreRetryable, setRestoreRetryable] = createSignal(false);
  // Stable per source and base: uncertain retries reuse the identical key.
  let restoreKey = "";
  let restoreKeyScope = "";
  const restoreIdempotencyKey = (baseRevisionId: string): string => {
    const scope =
      `${params.composition_id}/${params.revision_id}/${baseRevisionId}`;
    if (restoreKeyScope !== scope || !restoreKey) {
      restoreKeyScope = scope;
      restoreKey = crypto.randomUUID();
    }
    return restoreKey;
  };

  const handleRestore = async () => {
    const base = latestRevisionId();
    if (!base || restoring()) return;
    setRestoring(true);
    setRestoreError(null);
    setRestoreConflict(false);
    setRestoreRetryable(false);
    try {
      // Restore appends a new revision; existing history is never rewritten.
      const response = await compositionApi.restore(
        params.space_id,
        params.composition_id,
        params.revision_id,
        base,
        restoreIdempotencyKey(base),
      );
      navigate(
        spaceCompositionRevisionPath(
          params.space_id,
          response.composition_id,
          response.revision_id,
        ),
      );
    } catch (error) {
      if (
        error instanceof UgoiteApiError &&
        (error.status === 409 || error.code === "REVISION_CONFLICT")
      ) {
        setRestoreConflict(true);
        setRestoreError(t("composition.restoreConflict"));
      } else {
        setRestoreError(
          formatUserFacingError(
            error,
            "composition.restoreFailed",
            "composition.restore",
          ),
        );
        const outcome = error instanceof UgoiteApiError
          ? error.mutationOutcome
          : undefined;
        if (outcome !== "rejected") setRestoreRetryable(true);
      }
    } finally {
      setRestoring(false);
    }
  };

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
    resolveCompositionFieldName(forms(), formId, fieldId);
  const parameterMismatch = (parameterId: string) =>
    diagnostics().some((diagnostic) =>
      diagnostic.parameter_id === parameterId &&
      (diagnostic.code === "parameter_type_mismatch" ||
        diagnostic.code === "parameter_missing")
    );

  return (
    <>
      <div class="flex items-center gap-2">
        <h1>{name()}</h1>
        <IconLink
          icon="edit"
          label={t("composition.studioEdit")}
          href={spaceCompositionEditPath(
            params.space_id,
            params.composition_id,
            params.revision_id,
          )}
        />
        <IconLink
          icon="history"
          label={t("composition.history")}
          href={historyHref()}
        />
        <Show when={isHistorical()}>
          <IconButton
            icon="refresh"
            label={t("composition.restore")}
            disabled={restoring()}
            onClick={() => void handleRestore()}
          />
        </Show>
      </div>
      <Show when={restoring()}>
        <LocalBusyIndicator label={t("composition.restore")} />
      </Show>
      <Show when={restoreError()}>
        <p class="ui-text-danger" role="alert">{restoreError()}</p>
      </Show>
      <Show when={restoreConflict()}>
        <A href={historyHref()}>{t("composition.history")}</A>
      </Show>
      <Show when={restoreRetryable() && !restoring()}>
        <button
          class="ui-button ui-button-secondary"
          type="button"
          onClick={() => void handleRestore()}
        >
          {t("composition.retry")}
        </button>
      </Show>
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
