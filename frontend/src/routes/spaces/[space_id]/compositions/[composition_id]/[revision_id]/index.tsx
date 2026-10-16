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
import { DashboardFlowRenderer } from "~/components/DashboardFlowRenderer";
import { InputControl } from "~/components/composition/InputControl";
import { FieldStack, FieldStackRow } from "~/components/FieldStack";
import { IconButton } from "~/components/IconButton";
import { IconLink } from "~/components/IconLink";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { compositionVariableDisplayName } from "~/lib/composition-variable-name";
import { safeText } from "~/lib/display-value";
import { t } from "~/lib/i18n";
import { compositionApi, compositionDisplayName } from "~/lib/composition-api";
import { createCompositionQueryHandle } from "~/lib/composition-query-handle";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import { formatUserFacingError } from "~/lib/user-facing-error";
import {
  spaceCompositionEditPath,
  spaceCompositionHistoryPath,
  spaceCompositionRevisionPath,
} from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "compositions" });

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
  const metricQueryContext = () => {
    const identity = current().identity;
    return identity
      ? {
        spaceId: identity.spaceId,
        target: {
          kind: "saved_revision" as const,
          composition_id: identity.compositionId,
          revision_id: identity.revisionId,
        },
        variables: handle.variables(),
        invalidVariables: [...handle.invalidVariables()],
        dependencies: current().resolved?.plan?.dependencies,
        onEdit: () =>
          navigate(
            spaceCompositionEditPath(
              identity.spaceId,
              identity.compositionId,
              identity.revisionId,
            ),
          ),
      }
      : undefined;
  };
  const composition = () => current().composition;
  const name = () =>
    compositionDisplayName(
      composition()?.fields.name ?? current().summary?.name,
    );
  const definitions = () => current().resolved?.variable_definitions ?? [];
  const diagnostics = () => current().resolved?.diagnostics ?? [];
  const variableMismatch = (variableId: string) =>
    handle.invalidVariables().has(variableId) ||
    diagnostics().some((diagnostic) =>
      diagnostic.variable_id === variableId &&
      (diagnostic.code === "variable_type_mismatch" ||
        diagnostic.code === "variable_missing")
    );

  // First-class flow layout for saved Tools. The stored spec YAML is
  // normalized side-effect-free (same as the edit route); rows, text
  // content, and variable-bound Input placement come from that document while values
  // and pages stay with the resolve plan and transient Work state. When
  // the layout is unavailable the route falls back to the flat renderer
  // with every declared variable editable.
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
  const variableDisplayDefinitions = () => definitions();
  const flowRows = () => {
    const rows = layoutDocument()?.spec.layout?.rows;
    return Array.isArray(rows) ? rows : undefined;
  };
  const flowTexts = () => {
    const texts: Record<
      string,
      { text: string; style: "title" | "heading" | "body" | "caption" }
    > = {};
    const defaults = Object.fromEntries(
      variableDisplayDefinitions().flatMap((definition) =>
        definition.default === undefined
          ? []
          : [[definition.id, definition.default] as const]
      ),
    );
    const values = { ...defaults, ...handle.variables() };
    for (const component of layoutDocument()?.spec.components ?? []) {
      if (
        component.kind === "text" &&
        (component.style === "title" || component.style === "heading" ||
          component.style === "body" || component.style === "caption")
      ) {
        texts[component.id] = {
          text: component.content.kind === "literal"
            ? component.content.text
            : safeText(values[component.content.variable]),
          style: component.style,
        };
      }
    }
    return texts;
  };
  const inputVariables = () =>
    Object.fromEntries(
      (layoutDocument()?.spec.components ?? []).flatMap((component) =>
        component.kind === "input"
          ? [[component.id, component.variable] as const]
          : []
      ),
    );
  const placedInputVariableIds = () => {
    const placed = new Set<string>();
    const inputByComponent = inputVariables();
    for (const row of flowRows() ?? []) {
      for (const item of row.items ?? []) {
        const variableId = inputByComponent[item.component ?? ""];
        if (variableId) placed.add(variableId);
      }
    }
    return placed;
  };
  const resolvedPlan = () =>
    current().resolved?.ok ? current().resolved?.plan : undefined;
  // Placed Inputs render in the flow before data resolution. This section
  // keeps only variables without a placed Input.
  const sectionDefinitions = () => {
    if (flowRows() === undefined) return definitions();
    const placed = placedInputVariableIds();
    return variableDisplayDefinitions().filter((definition) =>
      !placed.has(definition.id)
    );
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
          <h2>{t("composition.variables")}</h2>
          <FieldStack label={t("composition.variables")}>
            <For each={sectionDefinitions()}>
              {(definition) => (
                <FieldStackRow>
                  <InputControl
                    definition={definition}
                    displayName={compositionVariableDisplayName(
                      variableDisplayDefinitions(),
                      definition.id,
                    )}
                    value={handle.variables()[definition.id]}
                    rawValue={handle.variableInputs()[definition.id]?.raw}
                    onChange={(raw) =>
                      handle.setVariableRaw(definition.id, raw)}
                    invalid={variableMismatch(definition.id)}
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
          variableDefinitions={variableDisplayDefinitions()}
        />
      </Show>
      <Show when={current().resolving}>
        <LocalBusyIndicator label={t("composition.queryLoading")} />
      </Show>
      <Show when={flowRows()}>
        {(rows) => (
          <DashboardFlowRenderer
            layout={{ rows: rows() }}
            inputVariables={inputVariables()}
            plan={resolvedPlan() ?? { sources: [], component_bindings: [] }}
            texts={flowTexts()}
            variableDefinitions={variableDisplayDefinitions()}
            variableDisplayDefinitions={variableDisplayDefinitions()}
            variableValues={handle.variables()}
            variableInputs={handle.variableInputs()}
            variableInvalid={variableMismatch}
            onVariableInput={handle.setVariableRaw}
            sources={current().sources}
            metricQueryContext={metricQueryContext()}
            onNext={handle.next}
            onPrevious={handle.previous}
            onRetry={handle.retry}
          />
        )}
      </Show>
      <Show
        when={flowRows() === undefined && current().resolved?.ok &&
          current().resolved?.plan}
      >
        {(plan) => (
          <CompositionRenderer
            plan={plan()}
            sources={current().sources}
            texts={flowTexts()}
            metricQueryContext={metricQueryContext()}
            onNext={handle.next}
            onPrevious={handle.previous}
            onRetry={handle.retry}
          />
        )}
      </Show>
    </>
  );
}
