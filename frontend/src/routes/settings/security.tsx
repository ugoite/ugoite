import { useSearchParams } from "@solidjs/router";
import { createEffect, createSignal, For, Show } from "solid-js";
import { authApi } from "~/lib/auth-api";
import { GlobalShell } from "~/components/GlobalShell";
import { createResource } from "~/lib/recoverable-resource";
import { t, type TranslationKey } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { formatDateTimeLabel } from "~/lib/date-format";
import { NodeAuditLogViewer } from "~/components/AuditLogViewer";

const credentialTabs = [
  ["passkeys", "securityPage.passkeys"],
  ["sessions", "securityPage.sessions"],
  ["audit", "securityPage.auditLog"],
] as const;

type CredentialTab = typeof credentialTabs[number][0];

const credentialTabFromSearch = (value: unknown): CredentialTab =>
  credentialTabs.some(([id]) => id === value)
    ? value as CredentialTab
    : "passkeys";

export default function SecuritySettingsRoute() {
  return (
    <GlobalShell
      title={`${t("securityPage.title")} / ${t("securityPage.credentials")}`}
    >
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">Ugoite</div>
          <h1>{t("securityPage.title")}</h1>
        </div>
      </div>
      <main class="settingsMain surface">
        <CredentialSettings />
      </main>
    </GlobalShell>
  );
}

export function CredentialSettings() {
  const [searchParams, setSearchParams] = useSearchParams();
  const [actionError, setActionError] = createSignal<string | null>(null);
  const [activeTab, setActiveTab] = createSignal<CredentialTab>(
    credentialTabFromSearch(searchParams.tab),
  );
  createEffect(() => setActiveTab(credentialTabFromSearch(searchParams.tab)));
  const selectTab = (tab: CredentialTab) => {
    setActiveTab(tab);
    setSearchParams({ tab });
  };
  const runAction = async (action: () => Promise<void>) => {
    setActionError(null);
    try {
      await action();
    } catch (error) {
      setActionError(
        formatUserFacingError(error, "securityPage.actionFailed"),
      );
    }
  };
  const [credentials, { refetch }] = createResource(
    () => activeTab() === "audit" ? null : "credentials",
    async () => ({
      passkeys: await authApi.listPasskeys(),
      sessions: await authApi.listSessions(),
    }),
  );
  return (
    <>
      <h2>{t("securityPage.credentials")}</h2>
      <div
        class="tabs"
        role="tablist"
        aria-label={t("securityPage.credentialSettings")}
      >
        <For each={credentialTabs}>
          {([id, label]) => (
            <button
              type="button"
              role="tab"
              id={`credential-tab-${id}`}
              aria-selected={activeTab() === id}
              aria-controls={`credential-panel-${id}`}
              class="tab"
              classList={{ active: activeTab() === id }}
              onClick={() => selectTab(id)}
            >
              {t(label as TranslationKey)}
            </button>
          )}
        </For>
      </div>
      <Show when={credentials.error}>
        <p class="ui-alert ui-alert-error">
          {formatUserFacingError(
            credentials.error,
            "securityPage.failedLoad",
          )}
        </p>
      </Show>
      <Show when={actionError()}>
        <p class="ui-alert ui-alert-error" role="alert">{actionError()}</p>
      </Show>
      <Show when={activeTab() === "passkeys"}>
        <section
          id="credential-panel-passkeys"
          role="tabpanel"
          aria-labelledby="credential-tab-passkeys"
          class="ui-card ui-stack-sm"
        >
          <div class="flex items-center justify-between">
            <h2 class="text-lg font-semibold">{t("securityPage.passkeys")}</h2>
            <button
              type="button"
              class="ui-button ui-button-primary"
              onClick={() =>
                void runAction(async () => {
                  await authApi.addPasskey();
                  await refetch();
                })}
            >
              {t("securityPage.addPasskey")}
            </button>
          </div>
          <Show when={credentials()}>
            {(value) => (
              <For each={value().passkeys}>
                {(credential) => (
                  <div class="flex items-center justify-between">
                    <span>
                      {String(credential.credential_id)} ·{" "}
                      {t("securityPage.lastUsed")} {credential.last_used_at
                        ? formatDateTimeLabel(credential.last_used_at)
                        : t("securityPage.never")}
                    </span>
                    <button
                      type="button"
                      class="ui-button ui-button-secondary"
                      onClick={() =>
                        void runAction(async () => {
                          await authApi.revokePasskey(
                            String(credential.credential_id),
                          );
                          await refetch();
                        })}
                    >
                      {t("settings.revoke")}
                    </button>
                  </div>
                )}
              </For>
            )}
          </Show>
        </section>
      </Show>
      <Show when={activeTab() === "sessions"}>
        <section
          id="credential-panel-sessions"
          role="tabpanel"
          aria-labelledby="credential-tab-sessions"
          class="ui-card ui-stack-sm"
        >
          <h2 class="text-lg font-semibold">
            {t("securityPage.browserSessions")}
          </h2>
          <Show when={credentials()}>
            {(value) => (
              <For each={value().sessions}>
                {(session) => (
                  <div class="flex items-center justify-between">
                    <span>
                      {String(session.session_id)} ·{" "}
                      {t("securityPage.lastSeen")} {session.last_seen_at
                        ? formatDateTimeLabel(session.last_seen_at)
                        : t("securityPage.never")}
                      {session.revoked_at
                        ? ` · ${t("securityPage.revoked")}`
                        : ""}
                    </span>
                    <Show when={!session.revoked_at}>
                      <button
                        type="button"
                        class="ui-button ui-button-secondary"
                        onClick={() =>
                          void runAction(async () => {
                            await authApi.revokeSession(
                              String(session.session_id),
                            );
                            await refetch();
                          })}
                      >
                        {t("settings.revoke")}
                      </button>
                    </Show>
                  </div>
                )}
              </For>
            )}
          </Show>
        </section>
      </Show>
      <Show when={activeTab() === "audit"}>
        <section
          id="credential-panel-audit"
          role="tabpanel"
          aria-labelledby="credential-tab-audit"
          class="ui-card ui-stack-sm"
        >
          <h2 class="text-lg font-semibold">{t("securityPage.auditLog")}</h2>
          <NodeAuditLogViewer />
        </section>
      </Show>
    </>
  );
}
