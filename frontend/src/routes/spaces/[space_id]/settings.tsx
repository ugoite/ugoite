import { A, useParams, useSearchParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { SpaceSettings } from "~/components/SpaceSettings";
import { SpaceShell } from "~/components/SpaceShell";
import { UiIcon, type UiIconName } from "~/components/UiIcon";
import { locale } from "~/lib/i18n";
import {
  portablePreferences,
  setColorModePreference,
  setLocalePreference,
  setPrimaryColorPreference,
  setUiThemePreference,
} from "~/lib/preferences-store";
import { spaceApi } from "~/lib/ugoite-client";
import type {
  AgentPrincipal,
  SpaceMember,
  SpacePatchPayload,
  StorageConnectionConfig,
} from "~/lib/types";

type Section =
  | "general"
  | "members"
  | "agents"
  | "credentials"
  | "storage"
  | "appearance";
const sections: Array<
  { id: Section; icon: UiIconName; en: string; ja: string }
> = [
  { id: "general", icon: "settings", en: "General", ja: "一般" },
  { id: "members", icon: "members", en: "Members", ja: "メンバー" },
  { id: "agents", icon: "agent", en: "Agents", ja: "エージェント" },
  { id: "credentials", icon: "credential", en: "Credentials", ja: "認証情報" },
  { id: "storage", icon: "storage", en: "Storage", ja: "ストレージ" },
  { id: "appearance", icon: "appearance", en: "Appearance", ja: "外観" },
];
const managedRoles = ["owner", "editor", "viewer"] as const;
type ManagedRole = typeof managedRoles[number];
type AgentMode = "autonomous" | "delegated" | "both";
const message = (error: unknown, fallback: string) =>
  error instanceof Error && error.message.trim() ? error.message : fallback;

export default function SpaceSettingsRoute() {
  const params = useParams<{ space_id: string }>();
  const [search, setSearch] = useSearchParams();
  const spaceId = () => params.space_id;
  const active = createMemo<Section>(() =>
    sections.some((section) => section.id === search.section)
      ? search.section as Section
      : "general"
  );
  const label = (section: typeof sections[number]) =>
    section[locale() === "ja" ? "ja" : "en"];
  const [space, { refetch }] = createResource(spaceId, spaceApi.get);
  const [members, { refetch: refetchMembers }] = createResource(
    spaceId,
    spaceApi.listMembers,
  );
  const [agents, { refetch: refetchAgents }] = createResource(
    spaceId,
    spaceApi.listAgents,
  );
  const [inviteLabel, setInviteLabel] = createSignal("");
  const [inviteRole, setInviteRole] = createSignal<ManagedRole>("viewer");
  const [inviteUrl, setInviteUrl] = createSignal("");
  const [memberError, setMemberError] = createSignal("");
  const [agentName, setAgentName] = createSignal("");
  const [agentDescription, setAgentDescription] = createSignal("");
  const [agentMode, setAgentMode] = createSignal<AgentMode>("autonomous");
  const [agentActions, setAgentActions] = createSignal("read");
  const [agentExpiresAt, setAgentExpiresAt] = createSignal("");
  const [agentPublicKey, setAgentPublicKey] = createSignal("");
  const [agentError, setAgentError] = createSignal("");

  const saveSpace = async (payload: SpacePatchPayload) => {
    await spaceApi.patch(spaceId(), payload);
    await refetch();
  };
  const testConnection = (config: StorageConnectionConfig) =>
    spaceApi.testConnection(spaceId(), { storage_config: config });
  const invite = async () => {
    if (!inviteLabel().trim()) {
      setMemberError("Invitation label is required.");
      return;
    }
    try {
      const result = await spaceApi.inviteMember(spaceId(), {
        label: inviteLabel().trim(),
        role: inviteRole(),
      });
      setInviteUrl(result.invitation_url);
      setInviteLabel("");
      setMemberError("");
      await refetchMembers();
    } catch (error) {
      setMemberError(message(error, "Failed to invite member."));
    }
  };
  const updateRole = async (principalId: string, role: ManagedRole) => {
    try {
      await spaceApi.updateMemberRole(spaceId(), principalId, { role });
      await refetchMembers();
    } catch (error) {
      setMemberError(message(error, "Failed to update role."));
    }
  };
  const revokeMember = async (principalId: string) => {
    try {
      await spaceApi.revokeMember(spaceId(), principalId);
      await refetchMembers();
    } catch (error) {
      setMemberError(message(error, "Failed to revoke member."));
    }
  };
  const createAgent = async () => {
    try {
      const actions = agentActions().split(",").map((value) => value.trim())
        .filter((value) =>
          ["read", "create", "update"].includes(value)
        ) as Array<"read" | "create" | "update">;
      if (!agentName().trim() || !agentExpiresAt() || !actions.length) {
        throw new Error("Name, expiry, and at least one action are required.");
      }
      await spaceApi.createAgent(spaceId(), {
        display_name: agentName().trim(),
        description: agentDescription().trim(),
        mode: agentMode(),
        public_key_jwk: JSON.parse(agentPublicKey()),
        granted_actions: actions,
        expires_at: new Date(agentExpiresAt()).toISOString(),
      });
      setAgentName("");
      setAgentDescription("");
      setAgentPublicKey("");
      setAgentError("");
      await refetchAgents();
    } catch (error) {
      setAgentError(message(error, "Failed to create agent."));
    }
  };
  const revokeAgent = async (id: string) => {
    try {
      await spaceApi.revokeAgent(spaceId(), id);
      await refetchAgents();
    } catch (error) {
      setAgentError(message(error, "Failed to revoke agent."));
    }
  };

  return (
    <SpaceShell
      spaceId={spaceId()}
      activeNavigation="settings"
      title={`Settings / ${
        sections.find((section) => section.id === active())?.en ?? "General"
      }`}
    >
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{space()?.name || spaceId()}</div>
          <h1>Settings</h1>
        </div>
      </div>
      <div class="settingsLayout">
        <aside class="settingsNav surface">
          <For each={sections}>
            {(section) => (
              <button
                type="button"
                classList={{ active: active() === section.id }}
                onClick={() => setSearch({ section: section.id })}
              >
                <UiIcon name={section.icon} />
                <span>{label(section)}</span>
              </button>
            )}
          </For>
        </aside>
        <main>
          <Show when={space.loading}>
            <div class="settingsMain surface ui-muted">Loading space…</div>
          </Show>
          <Show when={space.error}>
            <div class="ui-alert ui-alert-error">
              Failed to load space: {message(space.error, "Unknown error")}
            </div>
          </Show>
          <Show when={space()}>
            {(current) => (
              <>
                <Show when={active() === "general"}>
                  <div class="ui-stack">
                    <SpaceSettings
                      space={current()}
                      section="general"
                      onSave={saveSpace}
                      onTestConnection={testConnection}
                    />
                    <section class="settingsMain surface">
                      <h2>Language</h2>
                      <label>
                        Language<select
                          value={locale()}
                          onChange={(event) =>
                            void setLocalePreference(
                              event.currentTarget.value as "en" | "ja",
                            )}
                        >
                          <option value="en">English</option>
                          <option value="ja">日本語</option>
                        </select>
                      </label>
                    </section>
                  </div>
                </Show>
                <Show when={active() === "storage"}>
                  <SpaceSettings
                    space={current()}
                    section="storage"
                    onSave={saveSpace}
                    onTestConnection={testConnection}
                  />
                </Show>
              </>
            )}
          </Show>

          <Show when={active() === "members"}>
            <section class="settingsMain surface">
              <h2>Members</h2>
              <div class="settingsGrid">
                <label>
                  Invitation label<input
                    value={inviteLabel()}
                    onInput={(e) => setInviteLabel(e.currentTarget.value)}
                  />
                </label>
                <label>
                  Role<select
                    value={inviteRole()}
                    onChange={(e) =>
                      setInviteRole(e.currentTarget.value as ManagedRole)}
                  >
                    <For each={managedRoles}>
                      {(role) => <option value={role}>{role}</option>}
                    </For>
                  </select>
                </label>
              </div>
              <button
                class="btn primary"
                type="button"
                onClick={() => void invite()}
              >
                Invite
              </button>
              <Show when={inviteUrl()}>
                <p class="ui-alert ui-alert-success">
                  Invitation URL: <code>{inviteUrl()}</code>
                </p>
              </Show>
              <Show when={memberError()}>
                <p class="ui-alert ui-alert-error">{memberError()}</p>
              </Show>
              <div class="rowStack">
                <For
                  each={members() ?? []}
                  fallback={<p class="ui-muted">No members found.</p>}
                >
                  {(member: SpaceMember) => (
                    <div class="rowBtn">
                      <span class="glyph">
                        <UiIcon name="members" />
                      </span>
                      <span>
                        <b>{member.principal.display_name}</b>
                        <small>{member.principal.state}</small>
                      </span>
                      <span class="actions">
                        <select
                          value={member.role}
                          disabled={member.role === "owner"}
                          onChange={(e) =>
                            void updateRole(
                              member.principal.principal_id,
                              e.currentTarget.value as ManagedRole,
                            )}
                        >
                          <For each={managedRoles}>
                            {(role) => <option value={role}>{role}</option>}
                          </For>
                        </select>
                        <button
                          class="btn danger"
                          type="button"
                          disabled={member.role === "owner"}
                          onClick={() =>
                            void revokeMember(member.principal.principal_id)}
                        >
                          Revoke
                        </button>
                      </span>
                    </div>
                  )}
                </For>
              </div>
            </section>
          </Show>

          <Show when={active() === "agents"}>
            <section class="settingsMain surface">
              <h2>Agents</h2>
              <div class="settingsGrid">
                <label>
                  Name<input
                    value={agentName()}
                    onInput={(e) => setAgentName(e.currentTarget.value)}
                  />
                </label>
                <label>
                  Mode<select
                    value={agentMode()}
                    onChange={(e) =>
                      setAgentMode(e.currentTarget.value as AgentMode)}
                  >
                    <option value="autonomous">autonomous</option>
                    <option value="delegated">delegated</option>
                    <option value="both">both</option>
                  </select>
                </label>
                <label>
                  Granted actions<input
                    value={agentActions()}
                    onInput={(e) => setAgentActions(e.currentTarget.value)}
                  />
                </label>
                <label>
                  Expiry<input
                    type="datetime-local"
                    value={agentExpiresAt()}
                    onInput={(e) => setAgentExpiresAt(e.currentTarget.value)}
                  />
                </label>
              </div>
              <label>
                Description<textarea
                  value={agentDescription()}
                  onInput={(e) => setAgentDescription(e.currentTarget.value)}
                />
              </label>
              <label>
                Public JWK<textarea
                  class="mono"
                  value={agentPublicKey()}
                  onInput={(e) => setAgentPublicKey(e.currentTarget.value)}
                />
              </label>
              <button
                class="btn primary"
                type="button"
                onClick={() => void createAgent()}
              >
                Create Agent
              </button>
              <Show when={agentError()}>
                <p class="ui-alert ui-alert-error">{agentError()}</p>
              </Show>
              <div class="rowStack">
                <For
                  each={agents() ?? []}
                  fallback={<p class="ui-muted">No agents found.</p>}
                >
                  {(agent: AgentPrincipal) => (
                    <div class="rowBtn">
                      <span class="glyph">
                        <UiIcon name="agent" />
                      </span>
                      <span>
                        <b>{agent.display_name}</b>
                        <small>{agent.mode} · {agent.status}</small>
                      </span>
                      <button
                        class="btn danger"
                        type="button"
                        disabled={agent.status === "revoked"}
                        onClick={() => void revokeAgent(agent.agent_id)}
                      >
                        Revoke
                      </button>
                    </div>
                  )}
                </For>
              </div>
            </section>
          </Show>

          <Show when={active() === "credentials"}>
            <section class="settingsMain surface">
              <h2>Credentials</h2>
              <div class="tabs">
                <button type="button" class="tab active">Passkeys</button>
                <button type="button" class="tab">OIDC</button>
                <button type="button" class="tab">Sessions</button>
                <button type="button" class="tab">Recovery TOTP</button>
                <button type="button" class="tab">CLI / MCP</button>
              </div>
              <div class="rowStack">
                <A class="rowBtn" href="/settings/security">
                  <span class="glyph active">
                    <UiIcon name="credential" />
                  </span>
                  <span>
                    <b>Manage credentials</b>
                    <small>
                      Passkeys, browser sessions, recovery and devices
                    </small>
                  </span>
                  <span>›</span>
                </A>
              </div>
            </section>
          </Show>

          <Show when={active() === "appearance"}>
            <section class="settingsMain surface">
              <h2>Appearance</h2>
              <div class="settingsGrid">
                <label>
                  Theme<select
                    value={portablePreferences().ui_theme ?? "classic"}
                    onChange={(e) =>
                      void setUiThemePreference(
                        e.currentTarget.value as
                          | "classic"
                          | "materialize"
                          | "pop",
                      )}
                  >
                    <option value="classic">Classic</option>
                    <option value="materialize">Materialize</option>
                    <option value="pop">Pop</option>
                  </select>
                </label>
                <label>
                  Mode<select
                    value={portablePreferences().color_mode ?? "light"}
                    onChange={(e) =>
                      void setColorModePreference(
                        e.currentTarget.value as "light" | "dark",
                      )}
                  >
                    <option value="light">Light</option>
                    <option value="dark">Dark</option>
                  </select>
                </label>
                <label>
                  Accent<select
                    value={portablePreferences().primary_color ?? "violet"}
                    onChange={(e) =>
                      void setPrimaryColorPreference(
                        e.currentTarget.value as
                          | "violet"
                          | "blue"
                          | "emerald"
                          | "amber",
                      )}
                  >
                    <option value="violet">Violet</option>
                    <option value="blue">Blue</option>
                    <option value="emerald">Emerald</option>
                    <option value="amber">Amber</option>
                  </select>
                </label>
              </div>
            </section>
          </Show>
        </main>
      </div>
    </SpaceShell>
  );
}
