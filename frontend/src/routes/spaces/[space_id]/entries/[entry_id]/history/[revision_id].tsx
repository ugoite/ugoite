import { useNavigate, useParams } from "@solidjs/router";
import {
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  Show,
} from "solid-js";
import { BackLink } from "~/components/BackLink";
import { ButtonSpinner } from "~/components/ButtonSpinner";
import { FieldValuesView } from "~/components/fields/FieldValue";
import { formatDateTimeLabel } from "~/lib/date-format";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { parseEntryMarkdownPresentation } from "~/lib/entry-input";
import {
  actorDisplayNameLookup,
  resolveActorDisplayName,
  revisionActorId,
  revisionOperationLabel,
} from "~/lib/entry-history";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { t } from "~/lib/i18n";
import { entryApi, spaceApi } from "~/lib/ugoite-client";
import type { SpaceMember } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import { spaceEntryPath } from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "entries", title: "revision" });

/** Revision subtitle: shared locale-aware date plus a localized marker. */
export function formatRevisionSubtitle(
  value: string | number | null | undefined,
): string {
  return t("entryRevision.subtitle", {
    date: formatDateTimeLabel(value),
    marker: t("entryRevision.readOnly"),
  });
}

export default function SpaceEntryRevisionRoute() {
  const navigate = useNavigate();
  const params = useParams<
    { space_id: string; entry_id: string; revision_id: string }
  >();
  const spaceId = () => params.space_id;
  const entryId = () => params.entry_id;
  const revisionId = () => params.revision_id;
  const entryPath = () => spaceEntryPath(spaceId(), entryId());

  const [revision] = createResource(() =>
    entryApi.getRevision(spaceId(), entryId(), revisionId())
  );
  // Best-effort member directory for the actor display name (same view
  // model as the history rows). Raw actor identity stays in the advanced
  // technical disclosure below, never in primary content.
  const [members] = createResource(
    () => spaceId(),
    async (id): Promise<SpaceMember[]> => {
      try {
        return await spaceApi.listMembers(id);
      } catch {
        return [];
      }
    },
    { initialValue: [] as SpaceMember[] },
  );
  const actorLookup = createMemo(() =>
    actorDisplayNameLookup(
      (members() ?? []).map((member) => ({
        principal_id: member.principal.principal_id,
        display_name: member.principal.display_name,
      })),
    )
  );
  const actorName = createMemo(() =>
    revision()
      ? resolveActorDisplayName(revision()!, actorLookup())
      : t("entryHistory.unknownActor")
  );
  const actorId = createMemo(() =>
    revision() ? revisionActorId(revision()!) : null
  );
  const [restoreError, setRestoreError] = createSignal<string | null>(null);
  const [isRestoring, setIsRestoring] = createSignal(false);
  // Restore runs behind an explicit confirmation dialog (PR4): the dialog
  // states the append-only semantics before the mutation can run.
  const [restoreConfirmOpen, setRestoreConfirmOpen] = createSignal(false);
  let confirmButtonRef: HTMLButtonElement | undefined;
  let restoreButtonRef: HTMLButtonElement | undefined;

  createEffect(() => {
    if (restoreConfirmOpen()) {
      // Move focus into the dialog when it opens (POL-UI-007 orderly path).
      queueMicrotask(() => confirmButtonRef?.focus());
    }
  });
  onCleanup(() => {
    confirmButtonRef = undefined;
    restoreButtonRef = undefined;
  });

  const openRestoreConfirm = () => {
    if (!revision() || isRestoring()) return;
    setRestoreError(null);
    setRestoreConfirmOpen(true);
  };
  const closeRestoreConfirm = () => {
    if (isRestoring()) return;
    setRestoreConfirmOpen(false);
    // Return focus to the invoking control on dismiss.
    queueMicrotask(() => restoreButtonRef?.focus());
  };
  const reviewError = createMemo(() =>
    revision.error
      ? formatUserFacingError(
        revision.error,
        "entryRevision.loadError",
        "entry.revision",
      )
      : null
  );

  // The stored Markdown is the revision content authority; the shared
  // read-only renderer shows it as text (never disabled inputs — editing
  // controls belong to the FieldInput family alone).
  const parsedRevision = createMemo(() => {
    const markdown = revision()?.markdown ?? "";
    try {
      return parseEntryMarkdownPresentation(markdown);
    } catch {
      return { title: "", fields: {} as Record<string, string> };
    }
  });
  const revisionFields = createMemo(() =>
    Object.keys(parsedRevision().fields).map((name) => ({ name }))
  );

  const copyText = async (value: string) => {
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      // Clipboard is a progressive enhancement; the value stays visible.
    }
  };

  const handleRestore = async () => {
    if (!revision() || isRestoring()) return;
    setIsRestoring(true);
    setRestoreError(null);
    try {
      // Restore is an append-only mutation. The response carries the newly
      // current revision; navigating to the Entry route reopens that state.
      await entryApi.restore(spaceId(), entryId(), revisionId());
      setRestoreConfirmOpen(false);
      navigate(entryPath());
    } catch (error) {
      setRestoreError(
        formatUserFacingError(
          error,
          "entryRevision.restoreError",
          "entry.restore",
        ),
      );
    } finally {
      setIsRestoring(false);
    }
  };

  const handleDialogKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      closeRestoreConfirm();
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{t("entryRevision.eyebrow")}</div>
          <h1>{t("entryRevision.title")}</h1>
          <Show when={revision()}>
            {(selected) => (
              <p class="ui-page-subtitle revision-subtitle">
                {formatRevisionSubtitle(selected().timestamp)}
              </p>
            )}
          </Show>
        </div>
        <BackLink
          href={`${entryPath()}/history`}
          label={t("entryRevision.backToHistory")}
        />
      </div>

      {/* Panel-local spinner: rendered content stays mounted on refetch. */}
      <Show when={revision.loading}>
        <LocalBusyIndicator label={t("entryRevision.loading")} />
      </Show>
      <Show when={reviewError()}>
        <p class="ui-alert ui-alert-error">{reviewError()}</p>
      </Show>
      <Show when={revision()}>
        <div class="settingsMain" aria-busy={revision.loading || undefined}>
          <p class="ui-alert ui-alert-warning">
            {t("entryRevision.restoreNotice")}
          </p>
          <p class="text-sm ui-muted">
            {t("entryRevision.operation")}:{" "}
            {revisionOperationLabel(revision()!)}
            {" · "}
            {t("entryRevision.actor")}: {actorName()}
          </p>
          <FieldValuesView
            fields={revisionFields()}
            getValue={(name) => parsedRevision().fields[name] ?? ""}
          />

          <div class="revision-restore-row">
            <button
              ref={restoreButtonRef}
              type="button"
              class="btn primary ui-entry-history-restore"
              aria-label={t("entryRevision.restore")}
              aria-busy={isRestoring() || undefined}
              onClick={openRestoreConfirm}
              disabled={isRestoring()}
            >
              <Show when={isRestoring()}>
                <ButtonSpinner />
              </Show>
              {t("entryRevision.restoreShort")}
            </button>
          </div>
          <Show when={restoreError()}>
            <p class="ui-alert ui-alert-error">{restoreError()}</p>
          </Show>

          {
            /*
            Advanced disclosure only: raw revision and actor identifiers live
            here (copyable), never in primary rows or headings.
          */
          }
          <details class="revision-technical-details">
            <summary>{t("entryHistory.debugDetails")}</summary>
            <dl class="ui-entry-detail-list">
              <div>
                <dt>{t("entryHistory.revisionId")}</dt>
                <dd class="font-mono break-all">
                  {revision()?.revision_id}
                  <Show when={revision()?.revision_id}>
                    {(id) => (
                      <button
                        type="button"
                        class="ui-button ui-button-secondary ui-button-sm ml-2"
                        aria-label={`${t("common.copy")} ${id()}`}
                        title={t("common.copy")}
                        onClick={() => void copyText(id())}
                      >
                        {t("common.copy")}
                      </button>
                    )}
                  </Show>
                </dd>
              </div>
              <Show when={actorId()}>
                {(id) => (
                  <div>
                    <dt>{t("entryRevision.actor")}</dt>
                    <dd class="font-mono break-all">
                      {id()}
                      <button
                        type="button"
                        class="ui-button ui-button-secondary ui-button-sm ml-2"
                        aria-label={`${t("common.copy")} ${id()}`}
                        title={t("common.copy")}
                        onClick={() => void copyText(id())}
                      >
                        {t("common.copy")}
                      </button>
                    </dd>
                  </div>
                )}
              </Show>
            </dl>
          </details>
        </div>
      </Show>

      {
        /*
        Restore confirmation dialog (PR4): states the append-only semantics
        (a new history event is created; existing history is never
        rewritten) and only then runs the mutation.
      */
      }
      <Show when={restoreConfirmOpen()}>
        <div
          class="ui-backdrop"
          onClick={(event) => {
            if (event.target === event.currentTarget) closeRestoreConfirm();
          }}
        >
          <div
            class="ui-dialog ui-restore-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="restore-confirm-title"
            aria-describedby="restore-confirm-body"
            onKeyDown={handleDialogKeyDown}
          >
            <h2 id="restore-confirm-title" class="ui-dialog-title">
              {t("entryRevision.restoreConfirmTitle")}
            </h2>
            <p id="restore-confirm-body" class="ui-restore-dialog-body">
              {t("entryRevision.restoreConfirmBody")}
            </p>
            <div class="ui-dialog-actions">
              <button
                type="button"
                class="ui-button ui-button-secondary"
                disabled={isRestoring()}
                onClick={closeRestoreConfirm}
              >
                {t("common.cancel")}
              </button>
              <button
                ref={confirmButtonRef}
                type="button"
                class="ui-button ui-button-primary"
                aria-busy={isRestoring() || undefined}
                disabled={isRestoring()}
                onClick={() => void handleRestore()}
              >
                <Show when={isRestoring()}>
                  <ButtonSpinner />
                </Show>
                {t("entryRevision.restoreConfirm")}
              </button>
            </div>
            <Show when={restoreError()}>
              <p class="ui-alert ui-alert-error mt-3">{restoreError()}</p>
            </Show>
          </div>
        </div>
      </Show>
    </>
  );
}
