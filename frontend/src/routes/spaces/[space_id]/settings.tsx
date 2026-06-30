import { A, useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { SpaceSettings } from "~/components/SpaceSettings";
import { getDocsiteHref } from "~/lib/docsite-links";
import { spaceApi } from "~/lib/ugoite-client";
import type {
  AgentPrincipal,
  SpaceMember,
  SpacePatchPayload,
  StorageConnectionConfig,
} from "~/lib/types";

const localDevAuthGuideUrl = getDocsiteHref(
  "/docs/guide/local-dev-auth-login",
  "docs/guide/local-dev-auth-login.md",
);

const managedRoles = ["owner", "editor", "viewer"] as const;
type ManagedRole = (typeof managedRoles)[number];
type ManagedAgentMode = "autonomous" | "delegated" | "both";

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

export default function SpaceSettingsRoute() {
  const params = useParams<{ space_id: string }>();
  const spaceId = () => params.space_id;

  const [space, { refetch }] = createResource(async () => {
    return await spaceApi.get(spaceId());
  });
  const [members, { refetch: refetchMembers }] = createResource(async () => {
    return await spaceApi.listMembers(spaceId());
  });
  const [agents, { refetch: refetchAgents }] = createResource(async () => {
    return await spaceApi.listAgents(spaceId());
  });

  const [inviteLabel, setInviteLabel] = createSignal("");
  const [inviteRole, setInviteRole] = createSignal<ManagedRole>("viewer");
  const [inviteUrl, setInviteUrl] = createSignal("");
  const [memberActionError, setMemberActionError] = createSignal("");
  const [memberActionPending, setMemberActionPending] = createSignal(false);
  const [agentName, setAgentName] = createSignal("");
  const [agentDescription, setAgentDescription] = createSignal("");
  const [agentMode, setAgentMode] = createSignal<ManagedAgentMode>("autonomous");
  const [agentActions, setAgentActions] = createSignal("read");
  const [agentExpiresAt, setAgentExpiresAt] = createSignal("");
  const [agentPublicKey, setAgentPublicKey] = createSignal("");
  const [agentError, setAgentError] = createSignal("");
  const [agentCredential, setAgentCredential] = createSignal("");

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
    const label = inviteLabel().trim();
    if (!label) {
      setMemberActionError("Invitation label is required.");
      return;
    }
    setMemberActionPending(true);
    setMemberActionError("");
    setInviteUrl("");
    try {
      const response = await spaceApi.inviteMember(spaceId(), {
        label,
        role: inviteRole(),
      });
      setInviteUrl(response.invitation_url);
      setInviteLabel("");
      await refetchMembers();
    } catch (error) {
      setMemberActionError(toMessage(error) || "Failed to invite member.");
    } finally {
      setMemberActionPending(false);
    }
  };

  const updateRole = async (principalId: string, role: ManagedRole) => {
    setMemberActionPending(true);
    setMemberActionError("");
    try {
      await spaceApi.updateMemberRole(spaceId(), principalId, { role });
      await refetchMembers();
    } catch (error) {
      setMemberActionError(toMessage(error) || "Failed to update role.");
    } finally {
      setMemberActionPending(false);
    }
  };

  const revokeMember = async (principalId: string) => {
    setMemberActionPending(true);
    setMemberActionError("");
    try {
      await spaceApi.revokeMember(spaceId(), principalId);
      await refetchMembers();
    } catch (error) {
      setMemberActionError(toMessage(error) || "Failed to revoke member.");
    } finally {
      setMemberActionPending(false);
    }
  };

  const createAgent = async () => {
    setAgentError("");
    setAgentCredential("");
    try {
      const actions = agentActions().split(",").map((value) => value.trim())
        .filter((value) => ["read", "create", "update"].includes(value)) as
        Array<"read" | "create" | "update">;
      if (!agentName().trim() || !agentExpiresAt() || actions.length === 0) {
        throw new Error("Name, expiry, and at least one action are required.");
      }
      const result = await spaceApi.createAgent(spaceId(), {
        display_name: agentName().trim(),
        description: agentDescription().trim(),
        mode: agentMode(),
        public_key_jwk: JSON.parse(agentPublicKey()),
        granted_actions: actions,
        expires_at: new Date(agentExpiresAt()).toISOString(),
      });
      setAgentCredential(String(result.credential.credential_id ?? "registered"));
      setAgentName("");
      setAgentDescription("");
      setAgentPublicKey("");
      await refetchAgents();
    } catch (error) {
      setAgentError(toMessage(error) || "Failed to create agent.");
    }
  };

  const revokeAgent = async (agentId: string) => {
    setAgentError("");
    try {
      await spaceApi.revokeAgent(spaceId(), agentId);
      await refetchAgents();
    } catch (error) {
      setAgentError(toMessage(error) || "Failed to revoke agent.");
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

          <div class="grid grid-cols-1 md:grid-cols-2 gap-2 items-end">
            <label class="ui-stack-sm">
              <span class="text-xs ui-muted">Invitation label</span>
              <input
                class="ui-input"
                value={inviteLabel()}
                onInput={(event) => setInviteLabel(event.currentTarget.value)}
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
          </div>
          <button
            type="button"
            class="ui-button ui-button-primary w-fit"
            onClick={handleInvite}
            disabled={memberActionPending()}
          >
            Invite Member
          </button>

          <Show when={inviteUrl()}>
            <div class="ui-alert ui-alert-info text-sm">
              Invitation URL (share once): <code>{inviteUrl()}</code>
            </div>
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
                      <p class="font-medium">
                        {member.principal.display_name}
                      </p>
                      <p class="text-xs ui-muted">
                        state: {member.principal.state}
                      </p>
                    </div>
                    <div class="flex flex-wrap items-center gap-2">
                      <Show
                        when={member.role !== "owner"}
                        fallback={<span class="text-sm ui-muted">owner</span>}
                      >
                        <select
                          class="ui-select"
                          value={member.role}
                          onInput={(event) =>
                            void updateRole(
                              member.principal.principal_id,
                              event.currentTarget.value as ManagedRole,
                            )}
                          disabled={memberActionPending() ||
                            member.principal.state !== "active"}
                        >
                          <For each={managedRoles}>
                            {(role) => <option value={role}>{role}</option>}
                          </For>
                        </select>
                      </Show>
                      <button
                        type="button"
                        class="ui-button ui-button-secondary text-sm"
                        onClick={() =>
                          void revokeMember(member.principal.principal_id)}
                        disabled={memberActionPending() ||
                          member.role === "owner" ||
                          member.principal.state === "revoked"}
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
        <section class="ui-card ui-stack-sm">
          <h2 class="text-lg font-semibold">Agents</h2>
          <p class="text-sm ui-muted">
            Register an independent public key, bounded actions, mode, sponsor,
            and expiry. Agent delete, sharing, member, owner, and agent
            administration remain denied.
          </p>
          <input
            class="ui-input"
            placeholder="Agent name"
            value={agentName()}
            onInput={(event) => setAgentName(event.currentTarget.value)}
          />
          <input
            class="ui-input"
            placeholder="Description"
            value={agentDescription()}
            onInput={(event) => setAgentDescription(event.currentTarget.value)}
          />
          <div class="grid grid-cols-1 md:grid-cols-3 gap-2">
            <select
              class="ui-select"
              value={agentMode()}
              onInput={(event) =>
                setAgentMode(event.currentTarget.value as ManagedAgentMode)}
            >
              <option value="autonomous">autonomous</option>
              <option value="delegated">delegated</option>
              <option value="both">both</option>
            </select>
            <input
              class="ui-input"
              value={agentActions()}
              onInput={(event) => setAgentActions(event.currentTarget.value)}
              aria-label="Agent actions"
              placeholder="read,create,update"
            />
            <input
              class="ui-input"
              type="datetime-local"
              value={agentExpiresAt()}
              onInput={(event) => setAgentExpiresAt(event.currentTarget.value)}
              aria-label="Agent expiry"
            />
          </div>
          <textarea
            class="ui-input font-mono"
            rows="5"
            placeholder='Public JWK, for example {"kty":"EC","crv":"P-256",...}'
            value={agentPublicKey()}
            onInput={(event) => setAgentPublicKey(event.currentTarget.value)}
          />
          <button
            type="button"
            class="ui-button ui-button-primary w-fit"
            onClick={() => void createAgent()}
          >
            Register agent
          </button>
          <Show when={agentCredential()}>
            <p class="ui-alert ui-alert-success">
              Credential registered: <code>{agentCredential()}</code>
            </p>
          </Show>
          <Show when={agentError()}>
            <p class="ui-text-danger">{agentError()}</p>
          </Show>
          <For each={agents() || []}>
            {(agent: AgentPrincipal) => (
              <div class="ui-card flex items-center justify-between gap-2">
                <span>
                  {agent.display_name} · {agent.mode} · {agent.status} · expires{" "}
                  {agent.expires_at}
                </span>
                <button
                  type="button"
                  class="ui-button ui-button-secondary"
                  disabled={agent.status === "revoked"}
                  onClick={() => void revokeAgent(agent.agent_id)}
                >
                  Revoke
                </button>
              </div>
            )}
          </For>
        </section>
      </div>
    </SpaceShell>
  );
}
