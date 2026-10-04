import { useNavigate } from "@solidjs/router";
import {
  type Accessor,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  Show,
} from "solid-js";
import { SaveAsToolDialog } from "./SaveAsToolDialog";
import { UiIcon } from "./UiIcon";
import {
  buildEntryQueryComposition,
  buildEntryQueryCompositionDocument,
  type EntryQueryCompositionFieldSchemaEntry,
  type EntryQueryCompositionSource,
} from "~/lib/entry-query-composition";
import type { EntryProjection, EntryQuery } from "~/lib/entry-query";
import type { Form } from "~/lib/types";
import { compositionApi } from "~/lib/composition-api";
import { compositionSaveErrorMessage } from "~/lib/composition-save-error";
import {
  beginCompositionSaveRouteVisit,
  clearPendingCompositionSaveAttempt,
  getPendingCompositionSaveAttempt,
  isCurrentCompositionSaveRouteVisit,
  markPendingCompositionSaveAttemptUncertain,
  type PendingCompositionSaveAttempt,
  stagePendingCompositionSaveAttempt,
  subscribeToPendingCompositionSaveAttempt,
} from "~/lib/composition-save-attempt";
import { t } from "~/lib/i18n";

export interface EntryQuerySaveAsToolProps {
  spaceId: Accessor<string>;
  routePath: Accessor<string>;
  defaultName: Accessor<string>;
  query: Accessor<EntryQuery>;
  projection: Accessor<EntryProjection>;
  form: Accessor<Form | undefined>;
  knownForms: Accessor<readonly Form[]>;
}

interface EntryQuerySaveSeed {
  spaceId: string;
  routePath: string;
  source: EntryQueryCompositionSource;
  fieldSchema: EntryQueryCompositionFieldSchemaEntry[];
}

/**
 * Save-as-tool toolbar button for an EntryQuery view. The button stays
 * disabled with the builder reason as its accessible name while the
 * current view has no exact Composition grammar (e.g. All-scope search).
 * The save/retry flow mirrors the Saved SQL run route: in-memory Work
 * state keyed by an idempotency key, never persisted to browser storage.
 */
export function EntryQuerySaveAsTool(props: EntryQuerySaveAsToolProps) {
  const navigate = useNavigate();
  const [saveDialogOpen, setSaveDialogOpen] = createSignal(false);
  const [saveBusy, setSaveBusy] = createSignal(false);
  const [saveRetryAvailable, setSaveRetryAvailable] = createSignal(false);
  const [saveError, setSaveError] = createSignal<string | null>(null);
  const [rejectedSaveName, setRejectedSaveName] = createSignal<string | null>(
    null,
  );
  let saveSeed: EntryQuerySaveSeed | undefined;
  let pendingSave: PendingCompositionSaveAttempt | undefined;
  let rejectedSaveRoute:
    | Pick<PendingCompositionSaveAttempt, "spaceId" | "routePath">
    | undefined;
  let saveRouteVisitId: number | undefined;

  const buildResult = createMemo(() =>
    buildEntryQueryComposition({
      query: props.query(),
      projection: props.projection(),
      form: props.form(),
      knownForms: props.knownForms(),
    })
  );
  const canSave = () => buildResult().status === "ok";
  const buttonLabel = () => {
    const result = buildResult();
    return result.status === "ok"
      ? t("composition.saveAsTool")
      : `${t("composition.saveAsTool")}, ${t(result.reason)}`;
  };

  const currentSaveRoute = () => ({
    spaceId: props.spaceId(),
    routePath: props.routePath(),
  });

  const isCurrentSaveRoute = (attempt: {
    spaceId: string;
    routePath: string;
  }) =>
    props.spaceId() === attempt.spaceId &&
    props.routePath() === attempt.routePath;

  const isCurrentSaveVisit = (
    attempt: PendingCompositionSaveAttempt,
    visitId: number | undefined,
  ) =>
    isCurrentSaveRoute(attempt) &&
    isCurrentCompositionSaveRouteVisit(attempt, visitId);

  const clearSaveForStaleRoute = () => {
    setSaveDialogOpen(false);
    setSaveBusy(false);
    setSaveRetryAvailable(false);
    setSaveError(null);
    setRejectedSaveName(null);
    rejectedSaveRoute = undefined;
    saveSeed = undefined;
    pendingSave = undefined;
  };

  createEffect(() => {
    const route = currentSaveRoute();
    const routeVisitId = beginCompositionSaveRouteVisit(route);
    saveRouteVisitId = routeVisitId;
    if (
      (rejectedSaveRoute && !isCurrentSaveRoute(rejectedSaveRoute)) ||
      (saveSeed && !isCurrentSaveRoute(saveSeed)) ||
      (pendingSave && !isCurrentSaveRoute(pendingSave))
    ) {
      clearSaveForStaleRoute();
    }
    const restoreAttempt = (
      stored: NonNullable<ReturnType<typeof getPendingCompositionSaveAttempt>>,
    ) => {
      pendingSave = stored.attempt;
      setSaveDialogOpen(true);
      setSaveBusy(false);
      setSaveRetryAvailable(true);
      setSaveError(
        stored.state === "uncertain" ? t("composition.saveFailed") : null,
      );
    };
    const savedAttempt = getPendingCompositionSaveAttempt(route);
    if (savedAttempt) restoreAttempt(savedAttempt);
    const unsubscribe = subscribeToPendingCompositionSaveAttempt(
      route,
      (event) => {
        if (event.type === "pending") {
          if (event.stored.routeVisitId !== routeVisitId) return;
          restoreAttempt(event.stored);
          return;
        }
        if (event.routeVisitId !== routeVisitId) return;
        if (pendingSave?.idempotencyKey !== event.idempotencyKey) return;
        pendingSave = undefined;
        setSaveBusy(false);
        setSaveRetryAvailable(false);
        rejectedSaveRoute = event.outcome === "rejected" ? route : undefined;
        setRejectedSaveName(
          event.outcome === "rejected" ? event.attemptName : null,
        );
        setSaveError(
          event.outcome === "rejected" ? t("composition.saveFailed") : null,
        );
        if (!saveSeed) setSaveDialogOpen(false);
      },
    );
    onCleanup(unsubscribe);
  });

  const saveRequest = async (
    attempt: PendingCompositionSaveAttempt,
    visitId: number | undefined,
    isRetry = false,
  ) => {
    stagePendingCompositionSaveAttempt(attempt, visitId);
    try {
      const response = await compositionApi.save(
        attempt.spaceId,
        attempt.yaml,
        attempt.idempotencyKey,
      );
      if (!isCurrentSaveVisit(attempt, visitId)) {
        markPendingCompositionSaveAttemptUncertain(attempt, visitId);
        return;
      }
      clearPendingCompositionSaveAttempt(attempt, "completed", visitId);
      pendingSave = undefined;
      navigate(
        `/spaces/${encodeURIComponent(props.spaceId())}/compositions/${
          encodeURIComponent(response.composition_id)
        }/${encodeURIComponent(response.revision_id)}`,
      );
    } catch (error) {
      const outcome = error && typeof error === "object" &&
          "mutationOutcome" in error
        ? (error as { mutationOutcome?: unknown }).mutationOutcome
        : "unknown";
      const status = error && typeof error === "object" && "status" in error
        ? (error as { status?: unknown }).status
        : undefined;
      const retryWasDenied = isRetry && (status === 401 || status === 403);
      if (outcome === "rejected" && !retryWasDenied) {
        clearPendingCompositionSaveAttempt(attempt, "rejected", visitId);
        if (isCurrentSaveVisit(attempt, visitId)) {
          setSaveError(compositionSaveErrorMessage(error));
        }
      } else {
        markPendingCompositionSaveAttemptUncertain(attempt, visitId);
      }
    }
  };

  const openSaveDialog = () => {
    const result = buildResult();
    if (result.status !== "ok") return;
    saveSeed = {
      spaceId: props.spaceId(),
      routePath: props.routePath(),
      source: result.source,
      fieldSchema: result.fieldSchema,
    };
    pendingSave = undefined;
    setSaveRetryAvailable(false);
    setSaveDialogOpen(true);
  };

  const handleSaveAsTool = async (name: string) => {
    if (saveBusy() || saveRetryAvailable()) return;
    const seed = saveSeed;
    if (!seed || !saveDialogOpen()) return;
    const visitId = saveRouteVisitId;
    pendingSave = undefined;
    rejectedSaveRoute = undefined;
    setSaveError(null);
    setRejectedSaveName(null);
    setSaveBusy(true);
    try {
      const document = buildEntryQueryCompositionDocument(
        name,
        seed.source,
        seed.fieldSchema,
      );
      const canonical = await compositionApi.canonicalizeDocument(document);
      if (saveRouteVisitId !== visitId || !isCurrentSaveRoute(seed)) return;
      const attempt: PendingCompositionSaveAttempt = {
        spaceId: seed.spaceId,
        routePath: seed.routePath,
        name,
        yaml: canonical.canonical_yaml,
        idempotencyKey: crypto.randomUUID(),
      };
      pendingSave = attempt;
      await saveRequest(attempt, visitId);
    } catch {
      if (saveRouteVisitId === visitId) {
        setSaveError(t("composition.saveFailed"));
      }
    } finally {
      if (saveRouteVisitId === visitId) setSaveBusy(false);
    }
  };

  const handleRetrySave = async () => {
    if (saveBusy() || !saveRetryAvailable() || !pendingSave) return;
    const attempt = pendingSave;
    const visitId = saveRouteVisitId;
    setSaveError(null);
    setSaveBusy(true);
    try {
      await saveRequest(attempt, visitId, true);
    } finally {
      if (saveRouteVisitId === visitId) setSaveBusy(false);
    }
  };

  const closeSaveDialog = () => {
    if (saveBusy() || saveRetryAvailable()) return;
    setSaveDialogOpen(false);
    saveSeed = undefined;
    pendingSave = undefined;
    if (!rejectedSaveName()) setSaveError(null);
  };

  return (
    <>
      <button
        type="button"
        class="ui-button ui-button-secondary entry-browser-save-tool"
        aria-label={buttonLabel()}
        title={buttonLabel()}
        disabled={!canSave()}
        onClick={openSaveDialog}
      >
        <UiIcon name="save" />
      </button>
      <Show when={saveDialogOpen()}>
        <SaveAsToolDialog
          initialName={pendingSave?.name ?? rejectedSaveName() ??
            props.defaultName()}
          busy={saveBusy()}
          retryAvailable={saveRetryAvailable()}
          error={saveError()}
          onSave={(name) => void handleSaveAsTool(name)}
          onRetry={() => void handleRetrySave()}
          onClose={closeSaveDialog}
        />
      </Show>
    </>
  );
}
