import { useParams, useSearchParams } from "@solidjs/router";
import { createMemo, createSignal, For, Show } from "solid-js";
import { SpaceSettings } from "~/components/SpaceSettings";
import { SpaceShell } from "~/components/SpaceShell";
import { UiIcon } from "~/components/UiIcon";
import { CredentialSettings } from "~/routes/settings/security";
import { locale, t, type TranslationKey } from "~/lib/i18n";
import { setLocalePreference } from "~/lib/preferences-store";
import { spaceApi } from "~/lib/ugoite-client";
import type {
  AgentPrincipal,
  SpaceMember,
  SpacePatchPayload,
  StorageConnectionConfig,
} from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import {
  settingsSections,
  type SettingsSectionId,
} from "~/lib/settings-sections";
import { formatUserFacingError } from "~/lib/user-facing-error";

type Section = SettingsSectionId;
const managedRoles = ["owner", "editor", "viewer"] as const;
type ManagedRole = typeof managedRoles[number];
type AgentMode = "autonomous" | "delegated" | "both";
const message = (error: unknown, fallbackKey: TranslationKey) =>
  formatUserFacingError(error, fallbackKey);

const parseAgentPublicJwk = (
  value: string,
): Record<string, unknown> | null => {
  try {
    const parsed: unknown = JSON.parse(value);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return null;
    }
    return parsed as Record<string, unknown>;
  } catch {
    return null;
  }
};

export default function SpaceSettingsRoute() {
  const params = useParams<{ space_id: string }>();
  const [search, setSearch] = useSearchParams();
  const spaceId = () => params.space_id;
  const active = createMemo<Section>(() =>
    settingsSections.some((section) => section.id === search.section)
      ? search.section as Section
      : "general"
  );
  const label = (section: typeof settingsSections[number]) => t(section.key);
  const [space, { refetch }] = createResource(spaceId, spaceApi.get);
  const [members, { refetch: refetchMembers }] = createResource(
    () => active() === "members" ? spaceId() : null,
    spaceApi.listMembers,
  );
  const [agents, { refetch: refetchAgents }] = createResource(
    () => active() === "agents" ? spaceId() : null,
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
  const [agentCredential, setAgentCredential] = createSignal("");

  const saveSpace = async (payload: SpacePatchPayload) => {
    await spaceApi.patch(spaceId(), payload);
    await refetch();
  };
  const testConnection = (config: StorageConnectionConfig) =>
    spaceApi.testConnection(spaceId(), { storage_config: config });
  const invite = async () => {
    if (!inviteLabel().trim()) {
      setMemberError(t("settings.invitationLabelRequired"));
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
      setMemberError(message(error, "settings.failedInvite"));
    }
  };
  const updateRole = async (principalId: string, role: ManagedRole) => {
    try {
      await spaceApi.updateMemberRole(spaceId(), principalId, { role });
      await refetchMembers();
    } catch (error) {
      setMemberError(message(error, "settings.failedUpdateRole"));
    }
  };
  const revokeMember = async (principalId: string) => {
    try {
      await spaceApi.revokeMember(spaceId(), principalId);
      await refetchMembers();
    } catch (error) {
      setMemberError(message(error, "settings.failedRevokeMember"));
    }
  };
  const createAgent = async () => {
    setAgentError("");
    setAgentCredential("");
    try {
      const actions = agentActions().split(",").map((value) => value.trim())
        .filter((value) =>
          ["read", "create", "update"].includes(value)
        ) as Array<"read" | "create" | "update">;
      if (!agentName().trim() || !agentExpiresAt() || !actions.length) {
        throw new Error(t("settings.agentValidation"));
      }
      const publicJwk = parseAgentPublicJwk(agentPublicKey());
      if (!publicJwk) {
        setAgentError(t("settings.invalidPublicJwk"));
        return;
      }
      const result = await spaceApi.createAgent(spaceId(), {
        display_name: agentName().trim(),
        description: agentDescription().trim(),
        mode: agentMode(),
        public_key_jwk: publicJwk,
        granted_actions: actions,
        expires_at: new Date(agentExpiresAt()).toISOString(),
      });
      setAgentCredential(String(result.credential.credential_id ?? ""));
      setAgentName("");
      setAgentDescription("");
      setAgentPublicKey("");
      await refetchAgents();
    } catch (error) {
      setAgentError(message(error, "settings.failedCreateAgent"));
    }
  };
  const revokeAgent = async (id: string) => {
    try {
      await spaceApi.revokeAgent(spaceId(), id);
      await refetchAgents();
    } catch (error) {
      setAgentError(message(error, "settings.failedRevokeAgent"));
    }
  };

  return (
    <SpaceShell
      spaceId={spaceId()}
      activeNavigation="settings"
      title={`${t("settings.title")} / ${
        settingsSections.find((section) => section.id === active())
          ? label(settingsSections.find((section) => section.id === active())!)
          : t("settings.section.general")
      }`}
    >
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{space()?.name || spaceId()}</div>
          <h1>{t("settings.title")}</h1>
        </div>
      </div>
      <div class="settingsLayout">
        <aside class="settingsNav surface">
          <For each={settingsSections}>
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
            <div class="settingsMain surface ui-muted">
              {t("settings.loadingSpace")}
            </div>
          </Show>
          <Show when={space.error}>
            <div class="ui-alert ui-alert-error">
              {t("settings.failedLoadSpace", {
                error: message(space.error, "settings.unknownError"),
              })}
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
                      <h2>{t("settings.language")}</h2>
                      <label>
                        {t("settings.language")}
                        <select
                          value={locale()}
                          onChange={(event) =>
                            void setLocalePreference(
                              event.currentTarget.value as "en" | "ja",
                            )}
                        >
                          <option value="en">
                            {t("settings.language.english")}
                          </option>
                          <option value="ja">
                            {t("settings.language.japanese")}
                          </option>
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
              <h2>{t("settings.section.members")}</h2>
              <div class="settingsGrid">
                <label>
                  {t("settings.invitationLabel")}
                  <input
                    value={inviteLabel()}
                    onInput={(e) => setInviteLabel(e.currentTarget.value)}
                  />
                </label>
                <label>
                  {t("settings.role")}
                  <select
                    value={inviteRole()}
                    onChange={(e) =>
                      setInviteRole(e.currentTarget.value as ManagedRole)}
                  >
                    <For each={managedRoles}>
                      {(role) => (
                        <option value={role}>
                          {role} — {t(`settings.role.${role}` as TranslationKey)}
                        </option>
                      )}
                    </For>
                  </select>
                </label>
              </div>
              <button
                class="btn primary"
                type="button"
                onClick={() => void invite()}
              >
                {t("settings.invite")}
              </button>
              <Show when={inviteUrl()}>
                <p class="ui-alert ui-alert-success">
                  {t("settings.invitationUrl")}: <code>{inviteUrl()}</code>
                </p>
              </Show>
              <Show when={memberError()}>
                <p class="ui-alert ui-alert-error">{memberError()}</p>
              </Show>
              <Show when={members.loading}>
                <p class="ui-muted">{t("settings.loadingMembers")}</p>
              </Show>
              <Show when={members.error}>
                <p class="ui-alert ui-alert-error">
                  {t("settings.failedLoadMembers", {
                    error: message(members.error, "settings.unknownError"),
                  })}
                </p>
              </Show>
              <div class="rowStack">
                <For
                  each={members() ?? []}
                  fallback={
                    <Show when={!members.loading && !members.error}>
                      <p class="ui-muted">{t("settings.noMembers")}</p>
                    </Show>
                  }
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
                            {(role) => (
                              <option value={role}>
                                {role} — {t(`settings.role.${role}` as TranslationKey)}
                              </option>
                            )}
                          </For>
                        </select>
                        <button
                          class="btn danger"
                          type="button"
                          disabled={member.role === "owner"}
                          onClick={() =>
                            void revokeMember(member.principal.principal_id)}
                        >
                          {t("settings.revoke")}
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
              <h2>{t("settings.section.agents")}</h2>
              <div class="settingsGrid">
                <label>
                  {t("settings.agentName")}
                  <input
                    value={agentName()}
                    onInput={(e) => setAgentName(e.currentTarget.value)}
                  />
                </label>
                <label>
                  {t("settings.agentMode")}
                  <select
                    value={agentMode()}
                    onChange={(e) =>
                      setAgentMode(e.currentTarget.value as AgentMode)}
                  >
                    <option value="autonomous">
                      autonomous — {t("settings.agentMode.autonomous")}
                    </option>
                    <option value="delegated">
                      delegated — {t("settings.agentMode.delegated")}
                    </option>
                    <option value="both">
                      both — {t("settings.agentMode.both")}
                    </option>
                  </select>
                </label>
                <label>
                  {t("settings.grantedActions")}
                  <input
                    value={agentActions()}
                    onInput={(e) => setAgentActions(e.currentTarget.value)}
                  />
                </label>
                <label>
                  {t("settings.expiry")}
                  <input
                    type="datetime-local"
                    value={agentExpiresAt()}
                    onInput={(e) => setAgentExpiresAt(e.currentTarget.value)}
                  />
                </label>
              </div>
              <label>
                {t("settings.description")}
                <textarea
                  value={agentDescription()}
                  onInput={(e) => setAgentDescription(e.currentTarget.value)}
                />
              </label>
              <label>
                {t("settings.publicJwk")}
                <textarea
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
                {t("settings.createAgent")}
              </button>
              <Show when={agentError()}>
                <p class="ui-alert ui-alert-error">{agentError()}</p>
              </Show>
              <Show when={agentCredential()}>
                <p class="ui-alert ui-alert-success">
                  {t("settings.credentialRegistered")}:{" "}
                  <code>{agentCredential()}</code>
                </p>
              </Show>
              <Show when={agents.loading}>
                <p class="ui-muted">{t("settings.loadingAgents")}</p>
              </Show>
              <Show when={agents.error}>
                <p class="ui-alert ui-alert-error">
                  {t("settings.failedLoadAgents", {
                    error: message(agents.error, "settings.unknownError"),
                  })}
                </p>
              </Show>
              <div class="rowStack">
                <For
                  each={agents() ?? []}
                  fallback={
                    <Show when={!agents.loading && !agents.error}>
                      <p class="ui-muted">{t("settings.noAgents")}</p>
                    </Show>
                  }
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
                        {t("settings.revoke")}
                      </button>
                    </div>
                  )}
                </For>
              </div>
            </section>
          </Show>

          <Show when={active() === "credentials"}>
            <section class="settingsMain surface">
              <CredentialSettings />
            </section>
          </Show>
        </main>
      </div>
    </SpaceShell>
  );
}
