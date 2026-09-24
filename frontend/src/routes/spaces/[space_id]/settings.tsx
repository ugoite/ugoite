import { useParams, useSearchParams } from "@solidjs/router";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import {
  RowList,
  RowListButton,
  RowListItem,
  RowListLink,
} from "~/components/RowList";
import { UiIcon } from "~/components/UiIcon";
import { SpaceSettings } from "~/components/SpaceSettings";
import { SpaceAuditLogViewer } from "~/components/AuditLogViewer";
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
import { spaceHistoryPath } from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "settings" });

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
  const activeLabel = createMemo(() => {
    const current = settingsSections.find(
      (section) => section.id === active(),
    );
    return current ? label(current) : t("settings.title");
  });
  // Mobile settings navigation: the category list hides behind a menu
  // button and opens as a drawer. Selecting a category closes it.
  const [drawerOpen, setDrawerOpen] = createSignal(false);
  let menuButtonRef: HTMLButtonElement | undefined;
  let drawerCloseRef: HTMLButtonElement | undefined;
  const closeDrawer = (refocus = true) => {
    if (!drawerOpen()) return;
    setDrawerOpen(false);
    if (refocus) menuButtonRef?.focus();
  };
  const selectSection = (id: Section) => {
    setSearch({ section: id });
    closeDrawer();
  };
  // Never trap the drawer open across navigation: a deep link already
  // selects its category, so an open drawer would only obscure content.
  createEffect(() => {
    active();
    setDrawerOpen(false);
  });
  createEffect(() => {
    if (drawerOpen()) drawerCloseRef?.focus();
  });
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
      <div
        class="settingsLayout settingsWorkspace"
        classList={{ settingsNavOpen: drawerOpen() }}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            closeDrawer();
          }
        }}
      >
        <nav
          id="settings-nav"
          class="settingsNavPanel"
          aria-label={t("settings.title")}
        >
          <div class="settingsNavHead">
            <span class="text-sm font-semibold">{t("settings.title")}</span>
            <button
              ref={drawerCloseRef}
              type="button"
              class="ui-button ui-button-secondary ui-button-sm text-sm settingsNavClose"
              aria-label={t("spaceShell.closeMenu")}
              onClick={() => closeDrawer()}
            >
              <UiIcon name="close" />
            </button>
          </div>
          <RowList label={t("settings.title")}>
            <For each={settingsSections}>
              {(section) => (
                <RowListItem
                  main={section.id === "history"
                    ? (
                      <RowListLink
                        href={spaceHistoryPath(spaceId())}
                        primary={label(section)}
                        chevron
                      />
                    )
                    : (
                      <RowListButton
                        primary={label(section)}
                        chevron
                        ariaLabel={label(section)}
                        onActivate={() => selectSection(section.id)}
                      />
                    )}
                />
              )}
            </For>
          </RowList>
        </nav>
        <Show when={drawerOpen()}>
          <button
            type="button"
            class="drawerBackdrop"
            aria-label={t("spaceShell.closeMenu")}
            onClick={() => closeDrawer()}
          />
        </Show>
        <main>
          <h1 class="ui-sr-only">{t("settings.title")}</h1>
          <div class="settingsMenuRow">
            <button
              ref={menuButtonRef}
              type="button"
              class="ui-button ui-button-secondary text-sm settingsMenuButton"
              aria-label={t("settings.menu", { section: activeLabel() })}
              aria-expanded={drawerOpen()}
              aria-controls="settings-nav"
              onClick={() => setDrawerOpen((open) => !open)}
            >
              <UiIcon name="menu" />
              <span>{activeLabel()}</span>
            </button>
          </div>
          {/* Panel-local spinner: settings content stays mounted on refetch. */}
          <Show when={space.loading}>
            <div class="settingsMain surface" aria-busy="true">
              <LocalBusyIndicator label={t("settings.loadingSpace")} />
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
                        <span class="ui-sr-only">{t("settings.language")}</span>
                        <select
                          value={locale()}
                          aria-label={t("settings.language")}
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
                    <Show when={current().space_version}>
                      <p class="ui-muted">
                        <small>{current().space_version}</small>
                      </p>
                    </Show>
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
            <section
              class="settingsMain surface"
              aria-busy={members.loading || undefined}
            >
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
                <LocalBusyIndicator label={t("settings.loadingMembers")} />
              </Show>
              <Show when={members.error}>
                <p class="ui-alert ui-alert-error">
                  {t("settings.failedLoadMembers", {
                    error: message(members.error, "settings.unknownError"),
                  })}
                </p>
              </Show>
              <Show
                when={(members() ?? []).length > 0}
                fallback={
                  <Show when={!members.loading && !members.error}>
                    <p class="ui-muted">{t("settings.noMembers")}</p>
                  </Show>
                }
              >
                <div class="ui-table-wrapper overflow-x-auto">
                  <table class="ui-table membersTable">
                    <thead class="ui-table-head">
                      <tr>
                        <th class="ui-table-header-cell" scope="col">
                          {t("settings.member")}
                        </th>
                        <th class="ui-table-header-cell" scope="col">
                          {t("settings.role")}
                        </th>
                        <th class="ui-table-header-cell" scope="col">
                          {t("settings.memberState")}
                        </th>
                        <th class="ui-table-header-cell" scope="col">
                          {t("formTable.actions")}
                        </th>
                      </tr>
                    </thead>
                    <tbody class="ui-table-body">
                      <For each={members() ?? []}>
                        {(member: SpaceMember) => {
                          const displayName = member.principal.display_name
                            ?.trim();
                          return (
                            <tr class="ui-table-row">
                              <td class="ui-table-cell membersNameCell">
                                <span class="membersPrimary">
                                  {displayName ||
                                    t("common.untitled")}
                                </span>
                              </td>
                              <td class="ui-table-cell">
                                <select
                                  value={member.role}
                                  disabled={member.role === "owner"}
                                  aria-label={t("settings.role")}
                                  onChange={(e) =>
                                    void updateRole(
                                      member.principal.principal_id,
                                      e.currentTarget.value as ManagedRole,
                                    )}
                                >
                                  <For each={managedRoles}>
                                    {(role) => (
                                      <option value={role}>
                                        {role} — {t(
                                          `settings.role.${role}` as TranslationKey,
                                        )}
                                      </option>
                                    )}
                                  </For>
                                </select>
                              </td>
                              <td class="ui-table-cell">
                                {member.principal.state}
                              </td>
                              <td class="ui-table-cell">
                                <button
                                  class="btn danger"
                                  type="button"
                                  disabled={member.role === "owner"}
                                  onClick={() =>
                                    void revokeMember(
                                      member.principal.principal_id,
                                    )}
                                >
                                  {t("settings.revoke")}
                                </button>
                              </td>
                            </tr>
                          );
                        }}
                      </For>
                    </tbody>
                  </table>
                </div>
                <details class="settingsAdvanced">
                  <summary>{t("settings.advancedDetails")}</summary>
                  <dl class="ui-stack-sm">
                    <For each={members() ?? []}>
                      {(member: SpaceMember) => (
                        <div>
                          <dt class="ui-label">
                            {member.principal.display_name?.trim() ||
                              t("common.untitled")}
                          </dt>
                          <dd class="ui-muted">
                            <span class="ui-sr-only">
                              {t("settings.memberId")}:{" "}
                            </span>
                            <code>{member.principal.principal_id}</code>
                          </dd>
                        </div>
                      )}
                    </For>
                  </dl>
                </details>
              </Show>
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
          <Show when={active() === "history"}>
            <section class="settingsMain surface">
              <h2>{t("settings.section.history")}</h2>
              <a
                class="btn primary"
                href={spaceHistoryPath(spaceId())}
              >
                {t("settings.section.history")}
              </a>
            </section>
          </Show>
        </main>
      </div>
    </>
  );
}
