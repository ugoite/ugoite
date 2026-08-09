import { A, useNavigate } from "@solidjs/router";
import { GlobalShell } from "~/components/GlobalShell";
import { createMemo, createSignal, For, Show } from "solid-js";
import { getDocsiteHref } from "~/lib/docsite-links";
import { authApi, spaceApi } from "~/lib/ugoite-client";
import { sortSpaces } from "~/lib/space-list";
import type { Space } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";

const localDevAuthGuideUrl = getDocsiteHref(
  "/docs/guide/develop/local-dev-auth-login",
  "docs/guide/develop/local-dev-auth-login.md",
);
const browserWalkthroughUrl = getDocsiteHref(
  "/docs/guide/start/browser-first-entry",
  "docs/guide/start/browser-first-entry.md",
);

const normalizeCreateError = (value: unknown): string => {
  if (
    value instanceof UgoiteApiError &&
    value.code === "INVALID_IDENTIFIER"
  ) {
    return t("spacesPage.invalidSpaceId");
  }
  return formatUserFacingError(value, "spacesPage.failedCreate");
};

const isAuthenticationError = (value: unknown): boolean =>
  value instanceof UgoiteApiError &&
  (value.status === 401 || value.code === "AUTHENTICATION_FAILED");

const isForbiddenError = (value: unknown): boolean =>
  value instanceof UgoiteApiError &&
  (value.status === 403 || value.code === "FORBIDDEN");

function SpaceCards(props: { label: string; spaces: readonly Space[] }) {
  return (
    <ul aria-label={props.label} class="rowStack">
      <For each={props.spaces}>
        {(space) => (
          <li class="rowBtn">
            <span class="glyph active">
              {(space.name || space.id).slice(0, 1).toUpperCase()}
            </span>
            <span>
              <b>{space.name || space.id}</b>
              <small>ID: {space.id}</small>
            </span>
            <div class="flex flex-wrap gap-2">
              <A
                href={`/spaces/${space.id}/settings`}
                class="ui-button ui-button-secondary text-sm"
              >
                {t("spacesPage.openSettings")}
              </A>
              <A
                href={`/spaces/${space.id}/dashboard`}
                class="ui-button ui-button-primary text-sm"
              >
                {t("spacesPage.openSpace")}
              </A>
            </div>
          </li>
        )}
      </For>
    </ul>
  );
}

export default function SpacesIndexRoute() {
  const navigate = useNavigate();
  const [spacesError, setSpacesError] = createSignal<unknown>(null);
  const [spaces, { refetch: refetchSpaces }] = createResource(async () => {
    setSpacesError("");
    try {
      return await spaceApi.list();
    } catch (error) {
      setSpacesError(error);
      return [];
    }
  });
  const [showCreateForm, setShowCreateForm] = createSignal(false);
  const [newSpaceId, setNewSpaceId] = createSignal("");
  const [createError, setCreateError] = createSignal<string | null>(null);
  const [requiresPasskey, setRequiresPasskey] = createSignal(false);
  const [isCreating, setIsCreating] = createSignal(false);
  const listedSpaces = createMemo(() => sortSpaces(spaces() || []));

  const authHint = createMemo(
    (): { message: string; showGuide: boolean } | null => {
      if (isAuthenticationError(spacesError())) {
        return {
          message: t("spacesPage.authRequired"),
          showGuide: true,
        };
      }
      if (isForbiddenError(spacesError())) {
        return {
          message: t("spacesPage.authForbidden"),
          showGuide: false,
        };
      }
      return null;
    },
  );

  const hasNoSpaces = createMemo(
    () => !spaces.loading && !spacesError() && listedSpaces().length === 0,
  );

  const openCreateForm = () => {
    setCreateError(null);
    setRequiresPasskey(false);
    setShowCreateForm(true);
  };

  const closeCreateForm = () => {
    setShowCreateForm(false);
    setNewSpaceId("");
    setCreateError(null);
    setRequiresPasskey(false);
  };

  const createSpace = async (spaceId: string) => {
    const created = await spaceApi.create(spaceId);
    await refetchSpaces();
    closeCreateForm();
    navigate(`/spaces/${created.id}/dashboard`);
  };

  const handleCreateSpace = async (event: Event) => {
    event.preventDefault();
    const spaceId = newSpaceId().trim();
    if (!spaceId) {
      setCreateError(t("spacesPage.spaceIdRequired"));
      return;
    }
    setIsCreating(true);
    setCreateError(null);
    setRequiresPasskey(false);
    try {
      await createSpace(spaceId);
    } catch (error) {
      setRequiresPasskey(
        typeof error === "object" && error !== null &&
          (error as { code?: unknown }).code === "RECENT_PASSKEY_REQUIRED",
      );
      setCreateError(normalizeCreateError(error));
    } finally {
      setIsCreating(false);
    }
  };

  const reauthenticateAndCreate = async () => {
    const spaceId = newSpaceId().trim();
    if (!spaceId) return;
    setIsCreating(true);
    setCreateError(null);
    try {
      await authApi.loginWithPasskey();
      await createSpace(spaceId);
    } catch (error) {
      setCreateError(normalizeCreateError(error));
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <GlobalShell title={t("spacesPage.title")} active="spaces">
      <div class="ui-stack">
        <div class="screenHead">
          <div class="screenTitle">
            <div class="eyebrow">Ugoite</div>
            <h1>{t("spacesPage.title")}</h1>
          </div>
          <div class="actions">
            <A
              href="/spaces/join"
              class="ui-button ui-button-secondary text-sm"
            >
              {t("spacesPage.join")}
            </A>
            <Show when={!spacesError() && !showCreateForm() && !hasNoSpaces()}>
              <button
                type="button"
                class="ui-button ui-button-primary text-sm"
                onClick={openCreateForm}
              >
                {t("spacesPage.create")}
              </button>
            </Show>
            <A href="/" class="ui-muted text-sm">
              {t("spacesPage.backHome")}
            </A>
          </div>
        </div>

        <section class="settingsMain surface">
          <h2 class="text-lg font-semibold mb-3">
            {t("spacesPage.available")}
          </h2>
          <Show when={showCreateForm()}>
            <form class="ui-card ui-stack-sm mb-4" onSubmit={handleCreateSpace}>
              <div>
                <h3 class="text-base font-semibold">
                  {t("spacesPage.create")}
                </h3>
                <p class="text-sm ui-muted">
                  {t("spacesPage.createDescription")}
                </p>
              </div>
              <div class="ui-field">
                <label class="ui-label" for="space-name">
                  {t("spacesPage.spaceId")}
                </label>
                <input
                  id="space-name"
                  type="text"
                  class="ui-input"
                  value={newSpaceId()}
                  onInput={(event) => setNewSpaceId(event.currentTarget.value)}
                  placeholder={t("spacesPage.spaceIdPlaceholder")}
                />
                <p class="mt-2 text-xs ui-muted">
                  {t("spacesPage.spaceIdHelp")}
                </p>
              </div>
              <Show when={createError()}>
                <div class="ui-alert ui-alert-error text-sm" role="alert">
                  <p>{createError()}</p>
                  <Show when={requiresPasskey()}>
                    <button
                      type="button"
                      class="ui-button ui-button-secondary ui-button-sm mt-2"
                      onClick={() => void reauthenticateAndCreate()}
                      disabled={isCreating()}
                    >
                      {t("spacesPage.authenticate")}
                    </button>
                  </Show>
                </div>
              </Show>
              <div class="flex flex-wrap justify-end gap-2">
                <button
                  type="button"
                  class="ui-button ui-button-secondary text-sm"
                  onClick={closeCreateForm}
                  disabled={isCreating()}
                >
                  {t("spacesPage.cancel")}
                </button>
                <button
                  type="submit"
                  class="ui-button ui-button-primary text-sm"
                  disabled={!newSpaceId().trim() || isCreating()}
                >
                  {isCreating()
                    ? t("spacesPage.creating")
                    : t("spacesPage.create")}
                </button>
              </div>
            </form>
          </Show>
          <Show when={spaces.loading}>
            <p class="text-sm ui-muted">{t("spacesPage.loading")}</p>
          </Show>
          <Show when={spacesError()}>
            <p class="ui-alert ui-alert-error text-sm">
              {formatUserFacingError(
                spacesError(),
                "spacesPage.failedLoad",
              )}
            </p>
            <Show when={authHint()}>
              {(hint) => (
                <div class="ui-stack-sm mt-2">
                  <p class="text-sm ui-muted">{hint().message}</p>
                  <Show when={hint().showGuide}>
                    <a
                      href={localDevAuthGuideUrl}
                      target="_blank"
                      rel="noopener"
                      class="ui-muted text-sm hover:underline"
                    >
                      {t("spacesPage.localDevAuth")}
                    </a>
                  </Show>
                </div>
              )}
            </Show>
          </Show>
          <Show when={hasNoSpaces() && !showCreateForm()}>
            <div class="ui-card ui-card-dashed ui-stack-sm">
              <p class="text-sm ui-muted">{t("spacesPage.noSpaces")}</p>
              <div>
                <button
                  type="button"
                  class="ui-button ui-button-primary text-sm"
                  onClick={openCreateForm}
                >
                  {t("spacesPage.create")}
                </button>
              </div>
              <a
                href={browserWalkthroughUrl}
                target="_blank"
                rel="noopener"
                class="ui-muted text-sm hover:underline"
              >
                {t("spacesPage.learnFirstEntry")}
              </a>
            </div>
          </Show>
          <Show when={listedSpaces().length > 0}>
            <SpaceCards label={t("spacesPage.title")} spaces={listedSpaces()} />
          </Show>
        </section>
      </div>
    </GlobalShell>
  );
}
