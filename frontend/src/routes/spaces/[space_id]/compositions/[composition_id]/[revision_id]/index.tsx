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
  type CompositionFieldNames,
  CompositionRenderer,
} from "~/components/CompositionRenderer";
import { DashboardFlowRenderer } from "~/components/DashboardFlowRenderer";
import { ParameterControl } from "~/components/composition/ParameterControl";
import { FieldStack, FieldStackRow } from "~/components/FieldStack";
import { IconButton } from "~/components/IconButton";
import { IconLink } from "~/components/IconLink";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { t } from "~/lib/i18n";
import { compositionApi, compositionDisplayName } from "~/lib/composition-api";
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
 * Form rename cannot change which fields resolve. Unlabeled fields use the
 * matching source's saved schema order. Query semantics never depend on
 * this helper.
 */
export const resolveCompositionFieldName = (
  forms: readonly Form[] | undefined,
  formId: string,
  fieldId: number,
  fieldSchema?: readonly { field_id: number }[],
): string | undefined => {
  const field = Object.values(
    (forms ?? []).find((form) => form.id === formId)?.fields ?? {},
  ).find((definition) =>
    (definition.query_capability?.field.field_id ?? definition.id) === fieldId
  );
  const label = field?.label?.trim();
  if (label) return label;
  const index = fieldSchema?.findIndex((entry) => entry.field_id === fieldId) ??
    -1;
  return index < 0
    ? undefined
    : t("composition.studioFieldIndex", { index: index + 1 });
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
  const parameterMismatch = (parameterId: string) =>
    diagnostics().some((diagnostic) =>
      diagnostic.parameter_id === parameterId &&
      (diagnostic.code === "parameter_type_mismatch" ||
        diagnostic.code === "parameter_missing")
    );

  // First-class flow layout for saved Tools. The stored spec YAML is
  // normalized side-effect-free (same as the edit route); rows, text
  // content, and parameter placement come from that document while values
  // and pages stay with the resolve plan and transient Work state. When
  // the layout is unavailable the route falls back to the flat renderer
  // with every declared parameter editable.
  const specYaml = () => {
    const spec = current().composition?.fields?.["spec"];
    return typeof spec === "string" ? spec : undefined;
  };
  const [linted] = createResource(
    specYaml,
    (yaml) => compositionApi.lint(yaml).catch(() => undefined),
  );
  const layoutDocument = () => {
    const response = linted();
    if (!response || !response.ok || !response.value) return undefined;
    const document = response.value.document;
    if (
      document.format !== "ugoite.composition" ||
      document.kind !== "dashboard" || !Array.isArray(document.spec.components)
    ) return undefined;
    return document;
  };
  const fieldNames: CompositionFieldNames = (
    formId,
    fieldId,
    sourceId,
  ) => {
    const source = layoutDocument()?.spec.sources.find((entry) =>
      entry.id === sourceId && entry.kind === "entry_query" &&
      entry.form_id === formId
    );
    return resolveCompositionFieldName(
      forms(),
      formId,
      fieldId,
      source?.field_schema,
    );
  };
  const flowRows = () => {
    const rows = layoutDocument()?.spec.layout?.rows;
    return Array.isArray(rows) ? rows : undefined;
  };
  const flowTexts = () => {
    const texts: Record<
      string,
      { text: string; style: "title" | "heading" | "body" | "caption" }
    > = {};
    for (const component of layoutDocument()?.spec.components ?? []) {
      if (
        component.kind === "text" && "text" in component &&
        typeof component.text === "string" &&
        "style" in component &&
        (component.style === "title" || component.style === "heading" ||
          component.style === "body" || component.style === "caption")
      ) {
        texts[component.id] = { text: component.text, style: component.style };
      }
    }
    return texts;
  };
  const placedParameterIds = () => {
    const placed = new Set<string>();
    for (const row of flowRows() ?? []) {
      for (const item of row.items ?? []) {
        if (item.kind === "parameter" && item.parameter) {
          placed.add(item.parameter);
        }
      }
    }
    return placed;
  };
  const resolvedPlan = () =>
    current().resolved?.ok ? current().resolved?.plan : undefined;
  // Placed controls render inside the flow; the section below keeps only
  // unplaced (optional with defaults) definitions editable. Without a
  // resolved plan every declared parameter stays editable here so resolve
  // diagnostics keep their control.
  const sectionDefinitions = () => {
    if (flowRows() === undefined || resolvedPlan() === undefined) {
      return definitions();
    }
    const placed = placedParameterIds();
    return definitions().filter((definition) => !placed.has(definition.id));
  };

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
      <Show when={sectionDefinitions().length > 0}>
        <section class="section">
          <h2>{t("composition.parameters")}</h2>
          <FieldStack label={t("composition.parameters")}>
            <For each={sectionDefinitions()}>
              {(definition) => (
                <FieldStackRow>
                  <ParameterControl
                    definition={definition}
                    value={handle.parameters()[definition.id]}
                    onChange={(value) =>
                      handle.setParameter(definition.id, value)}
                    invalid={parameterMismatch(definition.id)}
                  />
                </FieldStackRow>
              )}
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
          <Show
            when={flowRows()}
            fallback={
              <CompositionRenderer
                plan={plan()}
                sources={current().sources}
                texts={flowTexts()}
                fieldNames={fieldNames}
                onNext={handle.next}
                onPrevious={handle.previous}
                onRetry={handle.retry}
              />
            }
          >
            {(rows) => (
              <DashboardFlowRenderer
                layout={{ rows: rows() }}
                plan={plan()}
                texts={flowTexts()}
                parameterDefinitions={definitions()}
                parameterValues={handle.parameters()}
                onParameterChange={(parameterId, value) =>
                  handle.setParameter(parameterId, value)}
                parameterInvalid={parameterMismatch}
                sources={current().sources}
                fieldNames={fieldNames}
                onNext={handle.next}
                onPrevious={handle.previous}
                onRetry={handle.retry}
              />
            )}
          </Show>
        )}
      </Show>
    </>
  );
}
