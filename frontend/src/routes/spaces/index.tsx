import { A, useNavigate } from "@solidjs/router";
import { ButtonSpinner } from "~/components/ButtonSpinner";
import { GlobalShell } from "~/components/GlobalShell";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { RowList, RowListItem, RowListLink } from "~/components/RowList";
import { UiIcon } from "~/components/UiIcon";
import { createMemo, createSignal, For, Show } from "solid-js";
import { getDocsiteHref } from "~/lib/docsite-links";
import { authApi, spaceApi } from "~/lib/ugoite-client";
import { sortSpaces, spaceUid } from "~/lib/space-list";
import type { Space } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";

const localDevAuthGuideUrl = getDocsiteHref(
  "/docs/develop/development-setup",
  "docs/develop/development-setup.md",
);
const browserWalkthroughUrl = getDocsiteHref(
  "/docs/get-started/quickstart",
  "docs/get-started/quickstart.mdx",
);

const normalizeCreateError = (value: unknown): string => {
  if (
    value instanceof UgoiteApiError &&
    value.code === "INVALID_IDENTIFIER"
  ) {
    return t("spacesPage.invalidSpaceSlug");
  }
  return formatUserFacingError(value, "spacesPage.failedCreate");
};

const isAuthenticationError = (value: unknown): boolean =>
  value instanceof UgoiteApiError &&
  (value.status === 401 || value.code === "AUTHENTICATION_FAILED");

const isForbiddenError = (value: unknown): boolean =>
  value instanceof UgoiteApiError &&
  (value.status === 403 || value.code === "FORBIDDEN");

function SpaceTable(props: {
  label: string;
  labelledBy: string;
  spaces: readonly Space[];
}) {
  return (
    <RowList label={props.label} labelledBy={props.labelledBy}>
      <For each={props.spaces}>
        {(space) => {
          const uid = spaceUid(space);
          return (
            <RowListItem
              main={
                <RowListLink
                  href={`/spaces/${encodeURIComponent(uid)}/dashboard`}
                  primary={space.name || space.slug || uid}
                  chevron
                />
              }
              actions={
                <A
                  href={`/spaces/${encodeURIComponent(uid)}/settings`}
                  class="rowListIconButton"
                  aria-label={t("spacesPage.openSettings")}
                  title={t("spacesPage.openSettings")}
                >
                  <UiIcon name="settings" />
                </A>
              }
            />
          );
        }}
      </For>
    </RowList>
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
  const [newSpaceName, setNewSpaceName] = createSignal("");
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
    setNewSpaceName("");
    setNewSpaceId("");
    setCreateError(null);
    setRequiresPasskey(false);
  };

  const createSpace = async (name: string, slug: string) => {
    const created = await spaceApi.create({ name, slug });
    // The server-returned Space UID is the authority for all later
    // operations. Never fall back to the requested slug or another
    // identifier: navigating by slug could open a different Space.
    const spaceUid = created.space_uid;
    if (!spaceUid) {
      throw new UgoiteApiError({
        kind: "invalid_arguments",
        code: "INVALID_INPUT",
        operation: "space.create",
        message: "Space creation response omitted space_uid",
        detail: { kind: "space_identity", field: "space_uid" },
      });
    }
    await refetchSpaces();
    closeCreateForm();
    navigate(`/spaces/${encodeURIComponent(spaceUid)}/dashboard`);
  };

  const handleCreateSpace = async (event: Event) => {
    event.preventDefault();
    const name = newSpaceName().trim();
    const slug = newSpaceId().trim();
    if (!name) {
      setCreateError(t("spacesPage.spaceNameRequired"));
      return;
    }
    if (!slug) {
      setCreateError(t("spacesPage.spaceSlugRequired"));
      return;
    }
    setIsCreating(true);
    setCreateError(null);
    setRequiresPasskey(false);
    try {
      await createSpace(name, slug);
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
    const name = newSpaceName().trim();
    const slug = newSpaceId().trim();
    if (!name || !slug) return;
    setIsCreating(true);
    setCreateError(null);
    try {
      await authApi.loginWithPasskey();
      await createSpace(name, slug);
    } catch (error) {
      setCreateError(normalizeCreateError(error));
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <GlobalShell active="spaces">
      <div class="ui-stack">
        <div class="screenHead">
          <div class="screenTitle">
            <h1 class="ui-sr-only" id="spaces-page-title">
              {t("spacesPage.title")}
            </h1>
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
                aria-label={t("spacesPage.newSpaceAria")}
                onClick={openCreateForm}
              >
                {t("spacesPage.createShort")}
              </button>
            </Show>
          </div>
        </div>

        <section
          class="settingsMain surface"
          aria-busy={spaces.loading || undefined}
        >
          <h2 class="text-lg font-semibold mb-3">
            {t("spacesPage.availableShort")}
          </h2>
          <Show when={showCreateForm()}>
            <form class="ui-card ui-stack-sm mb-4" onSubmit={handleCreateSpace}>
              <div class="ui-field">
                <label class="ui-label" for="space-display-name">
                  {t("spacesPage.spaceName")}
                </label>
                <input
                  id="space-display-name"
                  type="text"
                  class="ui-input"
                  value={newSpaceName()}
                  ref={(element) => element?.focus()}
                  onInput={(event) =>
                    setNewSpaceName(event.currentTarget.value)}
                  placeholder={t("spacesPage.spaceNamePlaceholder")}
                />
              </div>
              <div class="ui-field">
                <label class="ui-label" for="space-slug">
                  {t("spacesPage.spaceSlug")}
                </label>
                <input
                  id="space-slug"
                  type="text"
                  class="ui-input"
                  value={newSpaceId()}
                  onInput={(event) => setNewSpaceId(event.currentTarget.value)}
                  placeholder={t("spacesPage.spaceSlugPlaceholder")}
                />
                <p class="mt-2 text-xs ui-muted">
                  {t("spacesPage.spaceSlugHelp")}
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
                  disabled={!newSpaceName().trim() || !newSpaceId().trim() ||
                    isCreating()}
                  aria-busy={isCreating() || undefined}
                >
                  <Show when={isCreating()}>
                    <ButtonSpinner />
                  </Show>
                  {t("spacesPage.create")}
                </button>
              </div>
            </form>
          </Show>
          {/* Panel-local spinner: listed spaces stay mounted on refetch. */}
          <Show when={spaces.loading}>
            <LocalBusyIndicator label={t("spacesPage.loading")} />
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
                  aria-label={t("spacesPage.newSpaceAria")}
                  onClick={openCreateForm}
                >
                  {t("spacesPage.createShort")}
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
            <SpaceTable
              label={t("spacesPage.title")}
              labelledBy="spaces-page-title"
              spaces={listedSpaces()}
            />
          </Show>
        </section>
      </div>
    </GlobalShell>
  );
}
