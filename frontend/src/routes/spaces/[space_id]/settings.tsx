import { useParams, useSearchParams } from "@solidjs/router";
import { createMemo, createSignal, For, Show } from "solid-js";
import { SpaceSettings } from "~/components/SpaceSettings";
import { SpaceAuditLogViewer } from "~/components/AuditLogViewer";
import { UiIcon } from "~/components/UiIcon";
import { CredentialSettings } from "~/routes/settings/security";
import { locale, t, type TranslationKey } from "~/lib/i18n";
import { setLocalePreference } from "~/lib/preferences-store";
import { spaceApi } from "~/lib/ugoite-client";
import type {
  SpaceMember,
  SpacePatchPayload,
  StorageConnectionConfig,
} from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import {
  type SettingsSectionId,
  settingsSections,
} from "~/lib/settings-sections";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "settings", title: "settings" });

type Section = SettingsSectionId;
const managedRoles = ["owner", "editor", "viewer"] as const;
type ManagedRole = typeof managedRoles[number];
const message = (error: unknown, fallbackKey: TranslationKey) =>
  formatUserFacingError(error, fallbackKey);

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
  const [inviteLabel, setInviteLabel] = createSignal("");
  const [inviteRole, setInviteRole] = createSignal<ManagedRole>("viewer");
  const [inviteUrl, setInviteUrl] = createSignal("");
  const [memberError, setMemberError] = createSignal("");

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
  return (
    <>
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
                          {role} —{" "}
                          {t(`settings.role.${role}` as TranslationKey)}
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
                                {role} —{" "}
                                {t(`settings.role.${role}` as TranslationKey)}
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

          <Show when={active() === "credentials"}>
            <section class="settingsMain surface">
              <CredentialSettings />
            </section>
          </Show>
          <Show when={active() === "audit"}>
            <section class="settingsMain surface">
              <h2>{t("settings.section.audit")}</h2>
              <SpaceAuditLogViewer spaceId={spaceId()} />
            </section>
          </Show>
        </main>
      </div>
    </>
  );
}
