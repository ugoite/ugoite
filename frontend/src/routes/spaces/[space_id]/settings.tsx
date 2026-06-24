import { A, useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { SpaceSettings } from "~/components/SpaceSettings";
import { getDocsiteHref } from "~/lib/docsite-links";
import { spaceApi } from "~/lib/ugoite-client";
import type {
  SpaceMember,
  SpacePatchPayload,
  StorageConnectionConfig,
} from "~/lib/types";

const localDevAuthGuideUrl = getDocsiteHref(
  "/docs/guide/local-dev-auth-login",
  "docs/guide/local-dev-auth-login.md",
);

const managedRoles = ["admin", "editor", "viewer"] as const;
type ManagedRole = (typeof managedRoles)[number];

const invitationExpiryFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "short",
});

const toMessage = (value: unknown): string => {
  if (typeof value === "string" && value.trim()) return value;
  if (value instanceof Error && value.message.trim()) return value.message;
  return "";
};

const authHintFromError = (
  value: unknown,
): { message: string; showGuide: boolean } | null => {
  const message = toMessage(value).toLowerCase();
  if (
    message.includes("401") ||
    message.includes("authentication") ||
    message.includes("unauthorized")
  ) {
    return {
      message:
        "Authentication required. Open /login and sign in again for this local development session.",
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
        "You are signed in but do not have enough permissions for this action.",
      showGuide: false,
    };
  }
  return null;
};

const formatInvitationExpiry = (expiresAt: string) => {
  const date = new Date(expiresAt);
  return Number.isNaN(date.getTime())
    ? expiresAt
    : invitationExpiryFormatter.format(date);
};

const copyToClipboard = async (text: string) => {
  const clipboard = typeof navigator !== "undefined" ? navigator.clipboard : undefined;
  if (clipboard?.writeText) {
    try {
      await clipboard.writeText(text);
      return;
    } catch {
      // Fall back to the legacy clipboard path below.
    }
  }

  if (typeof document === "undefined" || !document.body) {
    throw new Error("Clipboard is unavailable in this environment.");
  }

  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "true");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.focus();
  textarea.select();
  const copied = typeof document.execCommand === "function" &&
    document.execCommand("copy");
  textarea.remove();

  if (!copied) {
    throw new Error("Copy to clipboard failed.");
  }
};

export default function SpaceSettingsRoute() {
  const params = useParams<{ space_id: string }>();
  const spaceId = () => params.space_id;

  const [space, { refetch }] = createResource(async () => {
    return await spaceApi.get(spaceId());
  });
  const [members, { refetch: refetchMembers }] = createResource(async () => {
    return await spaceApi.listMembers(spaceId());
  });

  const [inviteUserId, setInviteUserId] = createSignal("");
  const [inviteRole, setInviteRole] = createSignal<ManagedRole>("viewer");
  const [inviteEmail, setInviteEmail] = createSignal("");
  const [inviteDetails, setInviteDetails] = createSignal<{
    token: string;
    expiresAt: string;
  } | null>(null);
  const [inviteFeedback, setInviteFeedback] = createSignal<{
    kind: "success" | "error";
    message: string;
  } | null>(null);
  const [memberActionError, setMemberActionError] = createSignal("");
  const [memberActionPending, setMemberActionPending] = createSignal(false);

  const handleSave = async (payload: SpacePatchPayload) => {
    await spaceApi.patch(spaceId(), payload);
    await refetch();
  };

  const handleTestConnection = async (config: StorageConnectionConfig) => {
    return await spaceApi.testConnection(spaceId(), {
      storage_config: config,
    });
  };

  const handleInvite = async () => {
    const userId = inviteUserId().trim();
    if (!userId) {
      setMemberActionError("User ID is required.");
      return;
    }
    setMemberActionPending(true);
    setMemberActionError("");
    setInviteDetails(null);
    setInviteFeedback(null);
    try {
      const response = await spaceApi.inviteMember(spaceId(), {
        user_id: userId,
        role: inviteRole(),
        email: inviteEmail().trim() || undefined,
      });
      setInviteDetails({
        token: response.invitation.token,
        expiresAt: response.invitation.expires_at,
      });
      setInviteUserId("");
      setInviteEmail("");
      await refetchMembers();
    } catch (error) {
      setMemberActionError(toMessage(error) || "Failed to invite member.");
    } finally {
      setMemberActionPending(false);
    }
  };

  const handleCopyInviteToken = async () => {
    const details = inviteDetails();
    if (!details) return;
    try {
      await copyToClipboard(details.token);
      setInviteFeedback({
        kind: "success",
        message: "Invitation token copied to clipboard.",
      });
    } catch (error) {
      setInviteFeedback({
        kind: "error",
        message: toMessage(error) || "Failed to copy invitation token.",
      });
    }
  };

  const updateRole = async (memberUserId: string, role: ManagedRole) => {
    setMemberActionPending(true);
    setMemberActionError("");
    try {
      await spaceApi.updateMemberRole(spaceId(), memberUserId, { role });
      await refetchMembers();
    } catch (error) {
      setMemberActionError(toMessage(error) || "Failed to update role.");
    } finally {
      setMemberActionPending(false);
    }
  };

  const revokeMember = async (memberUserId: string) => {
    setMemberActionPending(true);
    setMemberActionError("");
    try {
      await spaceApi.revokeMember(spaceId(), memberUserId);
      await refetchMembers();
    } catch (error) {
      setMemberActionError(toMessage(error) || "Failed to revoke member.");
    } finally {
      setMemberActionPending(false);
    }
  };

  const spaceAuthHint = createMemo(() => {
    return authHintFromError(space.error);
  });
  const memberAuthHint = createMemo(() => {
    return authHintFromError(memberActionError() || members.error);
  });

  return (
    <SpaceShell spaceId={spaceId()}>
      <div class="mx-auto max-w-5xl ui-stack">
        <div>
          <h1 class="ui-page-title">Space Settings</h1>
          <p class="ui-page-subtitle mt-1">Space ID: {spaceId()}</p>
        </div>

        <div class="ui-card">
          <p class="text-sm ui-muted">
            Localhost and remote mode both require authenticated sessions. Start
            the dev stack with <code>mise run dev</code>, sign in at{" "}
            <code>/login</code> or through the CLI auth command, and follow{" "}
            <a
              href={localDevAuthGuideUrl}
              target="_blank"
              rel="noopener"
              class="hover:underline"
            >
              Local Dev Auth/Login
            </a>{" "}
            for the canonical workflow and local auth troubleshooting steps.
          </p>
        </div>

        <div class="mt-2">
          <Show when={space.loading}>
            <p class="text-sm ui-muted">Loading space...</p>
          </Show>
          <Show when={space.error}>
            <p class="text-sm ui-text-danger">Failed to load space.</p>
            <Show when={spaceAuthHint()}>
              {(hint) => (
                <div class="ui-stack-sm mt-1">
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
          <Show when={space()}>
            {(ws) => (
              <SpaceSettings
                space={ws()}
                onSave={handleSave}
                onTestConnection={handleTestConnection}
              />
            )}
          </Show>
        </div>

        <section class="ui-card ui-stack-sm">
          <h2 class="text-lg font-semibold">Members</h2>
          <p class="text-sm ui-muted">
            Invite members, update roles, and revoke access in this space.
          </p>

          <div class="grid grid-cols-1 md:grid-cols-4 gap-2 items-end">
            <label class="ui-stack-sm">
              <span class="text-xs ui-muted">User ID</span>
              <input
                class="ui-input"
                value={inviteUserId()}
                onInput={(event) => setInviteUserId(event.currentTarget.value)}
              />
            </label>
            <label class="ui-stack-sm">
              <span class="text-xs ui-muted">Role</span>
              <select
                class="ui-select"
                value={inviteRole()}
                onInput={(event) =>
                  setInviteRole(event.currentTarget.value as ManagedRole)}
              >
                <For each={managedRoles}>
                  {(role) => <option value={role}>{role}</option>}
                </For>
              </select>
            </label>
            <label class="ui-stack-sm md:col-span-2">
              <span class="text-xs ui-muted">Email (optional)</span>
              <input
                class="ui-input"
                value={inviteEmail()}
                onInput={(event) => setInviteEmail(event.currentTarget.value)}
              />
            </label>
          </div>
          <button
            type="button"
            class="ui-button ui-button-primary w-fit"
            onClick={handleInvite}
            disabled={memberActionPending()}
          >
            Invite Member
          </button>

          <Show when={inviteDetails()}>
            {(details) => (
              <div class="ui-alert ui-alert-info text-sm ui-stack-sm">
                <p class="font-medium">Invitation token (share once)</p>
                <div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                  <code class="break-all">{details().token}</code>
                  <button
                    type="button"
                    class="ui-button ui-button-secondary ui-button-sm w-fit"
                    onClick={() => void handleCopyInviteToken()}
                  >
                    Copy token
                  </button>
                </div>
                <p class="text-xs ui-muted">
                  Expires at{" "}
                  <time
                    datetime={details().expiresAt}
                    title={details().expiresAt}
                  >
                    {formatInvitationExpiry(details().expiresAt)}
                  </time>
                </p>
                <p class="text-xs ui-muted">
                  Treat this token as a secret and share it once.
                </p>
                <Show when={inviteFeedback()}>
                  {(feedback) => (
                    <p
                      class="text-xs"
                      role={feedback().kind === "error" ? "alert" : "status"}
                      aria-live="polite"
                    >
                      {feedback().message}
                    </p>
                  )}
                </Show>
              </div>
            )}
          </Show>
          <Show when={memberActionError()}>
            <p class="text-sm ui-text-danger">{memberActionError()}</p>
          </Show>
          <Show when={memberAuthHint()}>
            {(hint) => (
              <div class="ui-stack-sm">
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

          <Show when={members.loading}>
            <p class="text-sm ui-muted">Loading members...</p>
          </Show>
          <Show when={members.error}>
            <p class="text-sm ui-text-danger">
              Failed to load members
              <Show when={toMessage(members.error)}>
                {`: ${toMessage(members.error)}`}
              </Show>
            </p>
          </Show>
          <Show
            when={!members.loading && !members.error &&
              (members() || []).length === 0}
          >
            <p class="text-sm ui-muted">No members found.</p>
          </Show>
          <div class="ui-stack-sm">
            <For each={members() || []}>
              {(member: SpaceMember) => (
                <div class="ui-card flex flex-col gap-2">
                  <div class="flex flex-wrap items-center justify-between gap-2">
                    <div>
                      <p class="font-medium">{member.user_id}</p>
                      <p class="text-xs ui-muted">state: {member.state}</p>
                    </div>
                    <div class="flex flex-wrap items-center gap-2">
                      <Show
                        when={member.role !== "owner"}
                        fallback={<span class="text-sm ui-muted">owner</span>}
                      >
                        <select
                          class="ui-select"
                          value={member.role === "owner"
                            ? "admin"
                            : member.role}
                          onInput={(event) =>
                            void updateRole(
                              member.user_id,
                              event.currentTarget.value as ManagedRole,
                            )}
                          disabled={memberActionPending() ||
                            member.state !== "active"}
                        >
                          <For each={managedRoles}>
                            {(role) => <option value={role}>{role}</option>}
                          </For>
                        </select>
                      </Show>
                      <button
                        type="button"
                        class="ui-button ui-button-secondary text-sm"
                        onClick={() => void revokeMember(member.user_id)}
                        disabled={memberActionPending() ||
                          member.role === "owner" ||
                          member.state === "revoked"}
                      >
                        Revoke
                      </button>
                    </div>
                  </div>
                </div>
              )}
            </For>
          </div>
        </section>
      </div>
    </SpaceShell>
  );
}
