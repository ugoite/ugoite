import { A, useNavigate } from "@solidjs/router";
import { GlobalShell } from "~/components/GlobalShell";
import {
  createEffect,
  createMemo,
  createResource,
  createSignal,
  For,
  Show,
} from "solid-js";
import { getDocsiteHref } from "~/lib/docsite-links";
import { spaceApi } from "~/lib/ugoite-client";
import { sortSpaces } from "~/lib/space-list";
import type { Space } from "~/lib/types";

const localDevAuthGuideUrl = getDocsiteHref(
  "/docs/guide/local-dev-auth-login",
  "docs/guide/local-dev-auth-login.md",
);

const toMessage = (value: unknown): string => {
  if (value instanceof Error && value.message.trim()) {
    return value.message;
  }
  return "";
};

const normalizeCreateError = (value: unknown): string => {
  const message = toMessage(value);
  if (/invalid space_id:/i.test(message)) {
    return "Space IDs can use only letters, numbers, hyphens, and underscores.";
  }
  return message || "Failed to create space.";
};

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
                Settings
              </A>
              <A
                href={`/spaces/${space.id}/dashboard`}
                class="ui-button ui-button-primary text-sm"
              >
                Open Space
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
  const [spacesError, setSpacesError] = createSignal("");
  const [spaces, { refetch: refetchSpaces }] = createResource(async () => {
    setSpacesError("");
    try {
      return await spaceApi.list();
    } catch (error) {
      setSpacesError(toMessage(error) || "Failed to load spaces.");
      return [];
    }
  });
  const [redirected, setRedirected] = createSignal(false);
  const [showCreateForm, setShowCreateForm] = createSignal(false);
  const [newSpaceId, setNewSpaceId] = createSignal("");
  const [createError, setCreateError] = createSignal<string | null>(null);
  const [isCreating, setIsCreating] = createSignal(false);
  const listedSpaces = createMemo(() => sortSpaces(spaces() || []));

  const authHint = createMemo(
    (): { message: string; showGuide: boolean } | null => {
      const message = spacesError().toLowerCase();
      if (
        message.includes("401") ||
        message.includes("authentication") ||
        message.includes("unauthorized")
      ) {
        return {
          message:
            "Authentication required. Open /login to start a local browser session.",
          showGuide: true,
        };
      }
      if (
        message.includes("403") ||
        message.includes("forbidden") ||
        message.includes("not authorized")
      ) {
        return {
          message:
            "You are signed in but do not have permission to view these spaces.",
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
    setShowCreateForm(true);
  };

  const closeCreateForm = () => {
    setShowCreateForm(false);
    setNewSpaceId("");
    setCreateError(null);
  };

  const handleCreateSpace = async (event: Event) => {
    event.preventDefault();
    const spaceId = newSpaceId().trim();
    if (!spaceId) {
      setCreateError("Please provide a space ID.");
      return;
    }
    setIsCreating(true);
    setCreateError(null);
    try {
      const created = await spaceApi.create(spaceId);
      await refetchSpaces();
      closeCreateForm();
      navigate(`/spaces/${created.id}/dashboard`);
    } catch (error) {
      setCreateError(normalizeCreateError(error));
    } finally {
      setIsCreating(false);
    }
  };

  createEffect(() => {
    if (!spacesError() || redirected()) {
      return;
    }
    const message = spacesError().toLowerCase();
    if (
      message.includes("401") ||
      message.includes("authentication") ||
      message.includes("unauthorized")
    ) {
      setRedirected(true);
      navigate("/login?next=%2Fspaces");
    }
  });

  return (
    <GlobalShell title="Spaces" active="spaces">
      <div class="ui-stack">
        <div class="screenHead">
          <div class="screenTitle">
            <div class="eyebrow">Ugoite</div>
            <h1>Spaces</h1>
          </div>
          <div class="actions">
            <A
              href="/spaces/join"
              class="ui-button ui-button-secondary text-sm"
            >
              Join with invitation
            </A>
            <Show when={!spacesError() && !showCreateForm() && !hasNoSpaces()}>
              <button
                type="button"
                class="ui-button ui-button-primary text-sm"
                onClick={openCreateForm}
              >
                Create space
              </button>
            </Show>
            <A href="/" class="ui-muted text-sm">
              Back to Home
            </A>
          </div>
        </div>

        <section class="settingsMain surface">
          <h2 class="text-lg font-semibold mb-3">Available Spaces</h2>
          <Show when={showCreateForm()}>
            <form class="ui-card ui-stack-sm mb-4" onSubmit={handleCreateSpace}>
              <div>
                <h3 class="text-base font-semibold">Create space</h3>
                <p class="text-sm ui-muted">
                  Spaces are never created automatically. Choose a space ID to
                  create one explicitly.
                </p>
              </div>
              <div class="ui-field">
                <label class="ui-label" for="space-name">
                  Space ID
                </label>
                <input
                  id="space-name"
                  type="text"
                  class="ui-input"
                  value={newSpaceId()}
                  onInput={(event) => setNewSpaceId(event.currentTarget.value)}
                  placeholder="e.g. team-notes"
                />
                <p class="mt-2 text-xs ui-muted">
                  Use letters, numbers, hyphens, or underscores. This becomes
                  the space URL and storage ID.
                </p>
              </div>
              <Show when={createError()}>
                <p class="ui-alert ui-alert-error text-sm">{createError()}</p>
              </Show>
              <div class="flex flex-wrap justify-end gap-2">
                <button
                  type="button"
                  class="ui-button ui-button-secondary text-sm"
                  onClick={closeCreateForm}
                  disabled={isCreating()}
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  class="ui-button ui-button-primary text-sm"
                  disabled={!newSpaceId().trim() || isCreating()}
                >
                  {isCreating() ? "Creating..." : "Create space"}
                </button>
              </div>
            </form>
          </Show>
          <Show when={spaces.loading}>
            <p class="text-sm ui-muted">Loading spaces...</p>
          </Show>
          <Show when={spacesError()}>
            <p class="ui-alert ui-alert-error text-sm">
              Failed to load spaces.
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
                      Local Dev Auth/Login
                    </a>
                  </Show>
                </div>
              )}
            </Show>
          </Show>
          <Show when={hasNoSpaces() && !showCreateForm()}>
            <div class="ui-card ui-card-dashed ui-stack-sm">
              <p class="text-sm ui-muted">No spaces available.</p>
              <div>
                <button
                  type="button"
                  class="ui-button ui-button-primary text-sm"
                  onClick={openCreateForm}
                >
                  Create space
                </button>
              </div>
            </div>
          </Show>
          <Show when={listedSpaces().length > 0}>
            <SpaceCards label="Spaces" spaces={listedSpaces()} />
          </Show>
        </section>
      </div>
    </GlobalShell>
  );
}
