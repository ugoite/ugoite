import {
  createEffect,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  untrack,
} from "solid-js";
import { Portal } from "solid-js/web";
import { IconButton } from "~/components/IconButton";
import { RowList, RowListButton, RowListItem } from "~/components/RowList";
import { UiIcon } from "~/components/UiIcon";
import {
  bindCompositionComponent,
  type BindingComponentKind,
  commitResolvedCompositionBinding,
  type CompositionBindingProvider,
  type CompositionBindingSession,
  type CompositionFormAggregate,
  type CompositionFormChoice,
  type CompositionGraphDraft,
  type CompositionPreparedSavedSql,
  type CompositionQueryArgument,
  type CompositionSavedSqlChoice,
  compositionVariableFitsComponent,
  type CompositionVariableNode,
  isCompositionMetricType,
  rebindResolvedCompositionComponent,
  setCompositionComponentVariable,
} from "~/lib/composition-variable-binding";
import { t } from "~/lib/i18n";

type PickerTab = "variables" | "forms" | "saved_sql";
const aggregateKinds: CompositionFormAggregate[] = [
  "count",
  "sum",
  "avg",
  "min",
  "max",
];
const numericFieldTypes = new Set(["integer", "float", "decimal"]);
const aggregateFieldTypes = new Set([
  "string",
  "integer",
  "float",
  "decimal",
  "date",
  "timestamp",
]);

const formAggregateFields = (
  form: CompositionFormChoice,
  kind: CompositionFormAggregate | "",
) =>
  form.fields.filter((field) =>
    kind === "sum" || kind === "avg"
      ? numericFieldTypes.has(field.type)
      : aggregateFieldTypes.has(field.type)
  );

export interface CompositionVariableBindingPickerProps {
  kind: BindingComponentKind;
  draft: CompositionGraphDraft;
  forms: readonly CompositionFormChoice[];
  savedSql: readonly CompositionSavedSqlChoice[];
  provider: CompositionBindingProvider;
  session: CompositionBindingSession;
  getPrincipalGeneration: () => Promise<string>;
  isSessionCurrent: (
    session: CompositionBindingSession,
  ) => boolean | Promise<boolean>;
  savedSqlLoading?: boolean;
  savedSqlSeedUnavailable?: boolean;
  initialTab?: "variables" | "forms" | "saved_sql";
  /** Omit for insertion; supply the selected Table/Metric to rebind it. */
  componentId?: string;
  onCommit: (draft: CompositionGraphDraft) => void;
  onClose: () => void;
}

const nodeName = (node: CompositionVariableNode) => node.id;
const focusableSelector =
  'button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

const isVisibleFocusable = (element: HTMLElement) => {
  if (
    element.matches(':disabled, [aria-disabled="true"]') ||
    element.closest('[hidden], [inert], [aria-hidden="true"]')
  ) return false;
  let current: HTMLElement | null = element;
  while (current) {
    const style = window.getComputedStyle(current);
    if (style.display === "none" || style.visibility === "hidden") {
      return false;
    }
    current = current.parentElement;
  }
  return true;
};

/**
 * One modal owner for variable, Form, and exact Saved SQL selection. Query
 * resolution is delegated to the host; the picker commits the resolved node
 * and component reference together, then closes without opening an Inspector.
 */
export function CompositionVariableBindingPicker(
  props: CompositionVariableBindingPickerProps,
) {
  const titleId = "composition-variable-binding-title";
  const [tab, setTab] = createSignal<PickerTab>(
    props.initialTab ?? "variables",
  );
  const [working, setWorking] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [selectedForm, setSelectedForm] = createSignal<string | null>(null);
  const [aggregate, setAggregate] = createSignal<CompositionFormAggregate | "">(
    "",
  );
  const [aggregateColumn, setAggregateColumn] = createSignal("");
  const [preparedSavedSql, setPreparedSavedSql] = createSignal<
    {
      entryId: string;
      session: CompositionBindingSession;
      choices: CompositionPreparedSavedSql;
    } | null
  >(null);
  const [selectedSavedSql, setSelectedSavedSql] = createSignal<string | null>(
    null,
  );
  const [savedArgumentBindings, setSavedArgumentBindings] = createSignal<
    Record<string, string>
  >({});
  const [savedSqlInspection, setSavedSqlInspection] = createSignal<
    Record<
      string,
      { status: "loading" | "ready" | "unavailable"; reason?: string }
    >
  >({});
  let inspectionGeneration = 0;
  let dialog: HTMLDivElement | undefined;
  let opener: HTMLElement | null = null;

  onMount(() => {
    opener = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const appRoot = document.getElementById("app");
    appRoot?.setAttribute("inert", "");
    queueMicrotask(() => dialog?.querySelector("button")?.focus());
    onCleanup(() => {
      appRoot?.removeAttribute("inert");
      const target = opener;
      opener = null;
      queueMicrotask(() => target?.isConnected && target.focus());
    });
  });

  const close = () => {
    if (!working()) props.onClose();
  };

  const inspectSavedSqlCandidates = async () => {
    const inspect = props.provider.inspectSavedSql;
    if (!inspect) return;
    const generation = ++inspectionGeneration;
    for (const entry of props.savedSql) {
      if (savedSqlInspection()[entry.entry_id]?.status === "ready") continue;
      setSavedSqlInspection((current) => ({
        ...current,
        [entry.entry_id]: { status: "loading" },
      }));
      try {
        const session = {
          ...props.session,
          principal_generation: await props.getPrincipalGeneration(),
        };
        if (generation !== inspectionGeneration) return;
        const sessionIsCurrent = await props.isSessionCurrent(session);
        if (generation !== inspectionGeneration) return;
        if (!sessionIsCurrent) {
          props.onClose();
          return;
        }
        const prepared = await inspect({
          session,
          entry_id: entry.entry_id,
          revision_id: entry.revision_id,
          component: props.kind,
          draft: props.draft,
        });
        if (generation !== inspectionGeneration) return;
        const sessionMatches =
          session.session_id === props.session.session_id &&
          session.space_id === props.session.space_id &&
          session.document_revision === props.session.document_revision;
        const sessionStillCurrent = sessionMatches &&
          await props.isSessionCurrent(session);
        if (generation !== inspectionGeneration) return;
        if (!sessionStillCurrent) {
          props.onClose();
          return;
        }
        const available = props.kind === "table"
          ? prepared.table?.variable.returns.kind === "table"
          : prepared.scalars.some((candidate) => {
            const returns = candidate.binding.variable.returns;
            return returns.kind === "scalar" &&
              isCompositionMetricType(returns.value_type.scalar_type);
          });
        setSavedSqlInspection((current) => ({
          ...current,
          [entry.entry_id]: available ? { status: "ready" } : {
            status: "unavailable",
            reason: props.kind === "table"
              ? t("composition.savedSqlNotTable")
              : t("composition.savedSqlNoMetricValue"),
          },
        }));
      } catch {
        if (generation !== inspectionGeneration) return;
        setSavedSqlInspection((current) => ({
          ...current,
          [entry.entry_id]: {
            status: "unavailable",
            reason: t("composition.bindingUnavailable"),
          },
        }));
      }
    }
  };

  createEffect(() => {
    if (tab() === "saved_sql" && props.savedSql.length > 0) {
      void untrack(inspectSavedSqlCandidates);
    }
  });

  const savedSqlAvailability = (entry: CompositionSavedSqlChoice) =>
    savedSqlInspection()[entry.entry_id] ?? {
      status: props.provider.inspectSavedSql
        ? "loading" as const
        : "ready" as const,
    };

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
      return;
    }
    if (event.key !== "Tab") return;
    const controls = Array.from(
      (event.currentTarget as HTMLElement).querySelectorAll<HTMLElement>(
        focusableSelector,
      ),
    ).filter(isVisibleFocusable);
    if (controls.length === 0) {
      event.preventDefault();
      return;
    }
    const index = controls.indexOf(document.activeElement as HTMLElement);
    const next = event.shiftKey
      ? index <= 0 ? controls.length - 1 : index - 1
      : index < 0 || index === controls.length - 1
      ? 0
      : index + 1;
    event.preventDefault();
    controls[next].focus();
  };

  const commitVariable = (variableId: string) => {
    const next = props.componentId
      ? setCompositionComponentVariable(
        props.draft,
        props.componentId,
        variableId,
      )
      : bindCompositionComponent(props.draft, props.kind, variableId);
    if (!next) {
      setError(t("composition.bindingTypeMismatch"));
      return;
    }
    props.onCommit(next);
    props.onClose();
  };

  const commitResolved = async (
    resolve: (
      session: CompositionBindingSession,
    ) => ReturnType<CompositionBindingProvider["resolveForm"]>,
  ) => {
    if (working()) return;
    setWorking(true);
    setError(null);
    let session: CompositionBindingSession;
    try {
      session = {
        ...props.session,
        principal_generation: await props.getPrincipalGeneration(),
      };
      if (!await props.isSessionCurrent(session)) {
        props.onClose();
        return;
      }
      const resolved = await resolve(session);
      // The host checks Space, revision, and principal on the server and again
      // here before applying a response to the active authoring draft.
      if (
        session.session_id !== props.session.session_id ||
        session.space_id !== props.session.space_id ||
        session.document_revision !== props.session.document_revision ||
        !await props.isSessionCurrent(session)
      ) {
        props.onClose();
        return;
      }
      commitResolvedValue(resolved);
    } catch {
      setError(t("composition.bindingUnavailable"));
    } finally {
      setWorking(false);
    }
  };

  const commitResolvedValue = (
    resolved: Parameters<typeof commitResolvedCompositionBinding>[2],
  ) => {
    const next = props.componentId
      ? rebindResolvedCompositionComponent(
        props.draft,
        props.componentId,
        resolved,
      )
      : commitResolvedCompositionBinding(props.draft, props.kind, resolved);
    if (!next) {
      setError(t("composition.bindingTypeMismatch"));
      return false;
    }
    props.onCommit(next);
    props.onClose();
    return true;
  };

  const resolveForm = (form: CompositionFormChoice) =>
    commitResolved((session) =>
      props.provider.resolveForm({
        session,
        form_id: form.id,
        component: props.kind,
        ...(props.kind === "metric" && aggregate()
          ? {
            aggregate: {
              kind: aggregate() as CompositionFormAggregate,
              ...(aggregateColumn() ? { column: aggregateColumn() } : {}),
            },
          }
          : {}),
        draft: props.draft,
      })
    );

  const inspectSavedSql = async (entry: CompositionSavedSqlChoice) => {
    if (working()) return;
    setWorking(true);
    setError(null);
    setPreparedSavedSql(null);
    let session: CompositionBindingSession;
    try {
      session = {
        ...props.session,
        principal_generation: await props.getPrincipalGeneration(),
      };
      if (!await props.isSessionCurrent(session)) {
        props.onClose();
        return;
      }
      const choices = await props.provider.prepareSavedSql({
        session,
        entry_id: entry.entry_id,
        revision_id: entry.revision_id,
        component: props.kind,
        arguments: entry.arguments.map((argument) => ({
          name: argument.name,
          variable: savedArgumentBindings()[argument.name] ?? "",
        })),
        draft: props.draft,
      });
      if (
        session.session_id !== props.session.session_id ||
        session.space_id !== props.session.space_id ||
        session.document_revision !== props.session.document_revision ||
        !await props.isSessionCurrent(session)
      ) {
        props.onClose();
        return;
      }
      if (props.kind === "table") {
        if (!choices.table || choices.table.variable.returns.kind !== "table") {
          setError(t("composition.savedSqlNotTable"));
          return;
        }
        commitResolvedValue(choices.table);
        return;
      }
      const scalars = choices.scalars.filter((choice) => {
        const returns = choice.binding.variable.returns;
        return returns.kind === "scalar" &&
          isCompositionMetricType(returns.value_type.scalar_type);
      });
      if (scalars.length === 1) {
        commitResolvedValue(scalars[0].binding);
      } else if (scalars.length > 1) {
        setPreparedSavedSql({
          entryId: entry.entry_id,
          session,
          choices: { scalars },
        });
      } else {
        setError(t("composition.savedSqlNoMetricValue"));
      }
    } catch {
      setError(t("composition.bindingUnavailable"));
    } finally {
      setWorking(false);
    }
  };

  const commitPreparedScalar = async (
    choice: CompositionPreparedSavedSql["scalars"][number],
  ) => {
    const pending = preparedSavedSql();
    if (
      !pending || pending.session.session_id !== props.session.session_id ||
      pending.entryId !== selectedSavedSql()
    ) {
      props.onClose();
      return;
    }
    setWorking(true);
    setError(null);
    try {
      if (!await props.isSessionCurrent(pending.session)) {
        props.onClose();
        return;
      }
      commitResolvedValue(choice.binding);
    } catch {
      setError(t("composition.bindingUnavailable"));
    } finally {
      setWorking(false);
    }
  };

  const eligibleVariables = () =>
    props.draft.variables.filter((node) =>
      compositionVariableFitsComponent(node, props.kind)
    );

  const eligibleArgumentVariables = (type: string) =>
    props.draft.variables.filter((node) => {
      const valueType = node.kind === "query"
        ? node.returns.kind === "scalar" ? node.returns.value_type : undefined
        : node.value_type;
      return !!valueType && valueType.scalar_type === type &&
        type !== "decimal" && type !== "json";
    });

  const savedArgumentsComplete = (entry: CompositionSavedSqlChoice) =>
    entry.arguments.every((argument) => {
      const selected = savedArgumentBindings()[argument.name];
      return !!selected &&
        eligibleArgumentVariables(argument.type).some((node) =>
          node.id === selected
        );
    });

  const chooseSavedSql = (entry: CompositionSavedSqlChoice) => {
    setSelectedSavedSql(entry.entry_id);
    setPreparedSavedSql(null);
    setSavedArgumentBindings(
      Object.fromEntries(entry.arguments.map(({ name }) => [name, ""])),
    );
    setError(null);
    if (entry.arguments.length === 0) void inspectSavedSql(entry);
  };

  const tabLabel = (entry: PickerTab) =>
    entry === "variables"
      ? t("composition.studioVariables")
      : entry === "forms"
      ? t("composition.studioForms")
      : t("sqlPage.savedSql");

  return (
    <Portal>
      <div
        class="ui-backdrop"
        onClick={(event) => event.target === event.currentTarget && close()}
      >
        <section
          ref={dialog}
          class="ui-dialog compositionBindingPicker"
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          aria-busy={working() || undefined}
          onKeyDown={onKeyDown}
        >
          <header class="compositionBindingPickerHeader">
            <h2 id={titleId}>
              {props.kind === "table"
                ? t("composition.studioTable")
                : t("composition.studioMetric")}
            </h2>
            <IconButton
              icon="close"
              label={t("common.close")}
              onClick={close}
              disabled={working()}
            />
          </header>
          <div
            class="tabs compositionBindingPickerTabs"
            role="tablist"
            aria-label={t("composition.studioVariables")}
          >
            <For each={["variables", "forms", "saved_sql"] as const}>
              {(entry) => (
                <button
                  type="button"
                  role="tab"
                  id={`binding-tab-${entry}`}
                  aria-selected={tab() === entry}
                  aria-controls={`binding-panel-${entry}`}
                  classList={{ active: tab() === entry }}
                  disabled={working()}
                  onClick={() => {
                    if (tab() !== entry) inspectionGeneration++;
                    setTab(entry);
                    setError(null);
                    setPreparedSavedSql(null);
                    setSelectedSavedSql(null);
                  }}
                >
                  {tabLabel(entry)}
                </button>
              )}
            </For>
          </div>

          <div
            id="binding-panel-variables"
            role="tabpanel"
            aria-labelledby="binding-tab-variables"
            hidden={tab() !== "variables"}
          >
            <Show
              when={eligibleVariables().length > 0}
              fallback={<div class="compositionBindingPickerEmpty" />}
            >
              <RowList label={tabLabel("variables")}>
                <For each={eligibleVariables()}>
                  {(node) => (
                    <RowListItem
                      main={
                        <RowListButton
                          onActivate={() => commitVariable(node.id)}
                          disabled={working()}
                          primary={
                            <>
                              <UiIcon
                                name={node.kind === "input"
                                  ? "canvas-input"
                                  : node.kind === "query"
                                  ? "sql"
                                  : "appearance"}
                              />
                              <span class="rowListName">{nodeName(node)}</span>
                            </>
                          }
                        />
                      }
                    />
                  )}
                </For>
              </RowList>
            </Show>
          </div>

          <div
            id="binding-panel-forms"
            role="tabpanel"
            aria-labelledby="binding-tab-forms"
            hidden={tab() !== "forms"}
          >
            <RowList label={tabLabel("forms")}>
              <For each={props.forms}>
                {(form) => (
                  <RowListItem
                    main={props.kind === "table"
                      ? (
                        <RowListButton
                          onActivate={() => void resolveForm(form)}
                          disabled={working()}
                          primary={
                            <>
                              <UiIcon name="forms" />
                              <span class="rowListName">{form.name}</span>
                            </>
                          }
                        />
                      )
                      : (
                        <RowListButton
                          onActivate={() => {
                            setSelectedForm(form.id);
                            setAggregate("");
                            setAggregateColumn("");
                            setError(null);
                          }}
                          disabled={working()}
                          selected={selectedForm() === form.id}
                          primary={
                            <>
                              <UiIcon name="forms" />
                              <span class="rowListName">{form.name}</span>
                            </>
                          }
                        />
                      )}
                    actions={props.kind === "metric" &&
                        selectedForm() === form.id
                      ? (
                        <div class="compositionBindingFormControls">
                          <label>
                            <span class="ui-sr-only">
                              {t("composition.aggregateKind")}
                            </span>
                            <select
                              class="ui-input"
                              aria-label={t("composition.aggregateKind")}
                              value={aggregate()}
                              onChange={(event) => {
                                setAggregate(
                                  event.currentTarget.value as
                                    | CompositionFormAggregate
                                    | "",
                                );
                                setAggregateColumn("");
                              }}
                            >
                              <option value="">
                                {t("composition.chooseAggregate")}
                              </option>
                              <For each={aggregateKinds}>
                                {(kind) => (
                                  <option value={kind}>
                                    {kind.toUpperCase()}
                                  </option>
                                )}
                              </For>
                            </select>
                          </label>
                          <Show when={aggregate() && aggregate() !== "count"}>
                            <label>
                              <span class="ui-sr-only">
                                {t("composition.studioColumn")}
                              </span>
                              <select
                                class="ui-input"
                                aria-label={t("composition.studioColumn")}
                                value={aggregateColumn()}
                                onChange={(event) =>
                                  setAggregateColumn(event.currentTarget.value)}
                              >
                                <option value="">
                                  {t("composition.studioChooseColumn")}
                                </option>
                                <For
                                  each={formAggregateFields(
                                    form,
                                    aggregate(),
                                  )}
                                >
                                  {(field) => (
                                    <option value={field.name}>
                                      {field.name}
                                    </option>
                                  )}
                                </For>
                              </select>
                            </label>
                          </Show>
                          <Show
                            when={aggregate() && aggregate() !== "count" &&
                              formAggregateFields(form, aggregate()).length ===
                                0}
                          >
                            <span class="compositionBindingUnavailableReason">
                              {t("composition.formAggregateNoColumn")}
                            </span>
                          </Show>
                          <button
                            type="button"
                            class="btn primary"
                            disabled={working() || !aggregate() ||
                              (aggregate() !== "count" && !aggregateColumn())}
                            onClick={() => void resolveForm(form)}
                          >
                            {t("composition.studioUseSelection")}
                          </button>
                        </div>
                      )
                      : undefined}
                  />
                )}
              </For>
            </RowList>
          </div>

          <div
            id="binding-panel-saved_sql"
            role="tabpanel"
            aria-labelledby="binding-tab-saved_sql"
            hidden={tab() !== "saved_sql"}
          >
            <Show when={props.savedSqlLoading}>
              <div class="compositionBindingPickerEmpty" aria-hidden="true" />
            </Show>
            <Show when={props.savedSqlSeedUnavailable}>
              <p class="compositionBindingUnavailableReason" role="status">
                {t("composition.bindingUnavailable")}
              </p>
            </Show>
            <RowList label={tabLabel("saved_sql")}>
              <For each={props.savedSql}>
                {(entry) => (
                  <RowListItem
                    main={
                      <RowListButton
                        onActivate={() => chooseSavedSql(entry)}
                        disabled={working() ||
                          savedSqlAvailability(entry).status !== "ready"}
                        selected={selectedSavedSql() === entry.entry_id}
                        primary={
                          <>
                            <UiIcon name="sql" />
                            <span class="rowListName">{entry.name}</span>
                          </>
                        }
                      />
                    }
                    actions={savedSqlAvailability(entry).status ===
                        "unavailable"
                      ? (
                        <span class="compositionBindingUnavailableReason">
                          {savedSqlAvailability(entry).reason}
                        </span>
                      )
                      : selectedSavedSql() === entry.entry_id
                      ? (
                        <div class="compositionBindingSavedColumns">
                          <For each={entry.arguments}>
                            {(argument) => (
                              <label>
                                <span class="ui-sr-only">
                                  {t("composition.sqlArgumentBinding", {
                                    name: argument.name,
                                  })}
                                </span>
                                <select
                                  class="ui-input"
                                  aria-label={t(
                                    "composition.sqlArgumentBinding",
                                    { name: argument.name },
                                  )}
                                  value={savedArgumentBindings()[
                                    argument.name
                                  ] ?? ""}
                                  onChange={(event) =>
                                    setSavedArgumentBindings((current) => ({
                                      ...current,
                                      [argument.name]:
                                        event.currentTarget.value,
                                    }))}
                                >
                                  <option value="">
                                    {t(
                                      "composition.chooseSqlArgumentVariable",
                                      {
                                        name: argument.name,
                                      },
                                    )}
                                  </option>
                                  <For
                                    each={eligibleArgumentVariables(
                                      argument.type,
                                    )}
                                  >
                                    {(node) => (
                                      <option value={node.id}>{node.id}</option>
                                    )}
                                  </For>
                                </select>
                              </label>
                            )}
                          </For>
                          <Show when={entry.arguments.length > 0}>
                            <button
                              class="btn primary"
                              type="button"
                              disabled={working() ||
                                !savedArgumentsComplete(entry)}
                              onClick={() => void inspectSavedSql(entry)}
                            >
                              {t("composition.studioUseSelection")}
                            </button>
                          </Show>
                          <For
                            each={preparedSavedSql()?.entryId === entry.entry_id
                              ? preparedSavedSql()!.choices.scalars
                              : []}
                          >
                            {(candidate) => (
                              <button
                                class="pill"
                                type="button"
                                disabled={working()}
                                onClick={() => commitPreparedScalar(candidate)}
                              >
                                {candidate.column}
                              </button>
                            )}
                          </For>
                        </div>
                      )
                      : undefined}
                  />
                )}
              </For>
            </RowList>
          </div>

          <Show when={working()}>
            <div class="compositionBindingPickerStatus" role="status">
              <span class="ui-sr-only">{t("common.loading")}</span>
            </div>
          </Show>
          <Show when={error()}>
            <p class="ui-text-danger" role="alert">{error()}</p>
          </Show>
        </section>
      </div>
    </Portal>
  );
}
