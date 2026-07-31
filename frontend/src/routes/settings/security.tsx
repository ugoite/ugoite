import { useSearchParams } from "@solidjs/router";
import { createEffect, createSignal, For, Show } from "solid-js";
import { authApi } from "~/lib/auth-api";
import { GlobalShell } from "~/components/GlobalShell";
import { createResource } from "~/lib/recoverable-resource";
import { t, type TranslationKey } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";

const credentialTabs = [
  ["passkeys", "securityPage.passkeys"],
  ["oidc", "securityPage.oidc"],
  ["sessions", "securityPage.sessions"],
  ["totp", "securityPage.recoveryTotp"],
  ["devices", "securityPage.cliMcp"],
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
  const [totp, setTotp] = createSignal<{
    secret: string;
    otpauth_uri: string;
  }>();
  const [totpCode, setTotpCode] = createSignal("");
  const [totpConfigured, setTotpConfigured] = createSignal(false);
  const [activeTab, setActiveTab] = createSignal<CredentialTab>(
    credentialTabFromSearch(searchParams.tab),
  );
  createEffect(() => setActiveTab(credentialTabFromSearch(searchParams.tab)));
  const selectTab = (tab: CredentialTab) => {
    setActiveTab(tab);
    setSearchParams({ tab });
  };
  const [credentials, { refetch }] = createResource(async () => ({
    passkeys: await authApi.listPasskeys(),
    sessions: await authApi.listSessions(),
    devices: await authApi.listDevices(),
    oidcProviders: await authApi.listOidcProviders(),
  }));
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
              onClick={async () => {
                await authApi.addPasskey();
                await refetch();
              }}
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
                      {t("securityPage.lastUsed")} {String(
                        credential.last_used_at ?? t("securityPage.never"),
                      )}
                    </span>
                    <button
                      type="button"
                      class="ui-button ui-button-secondary"
                      onClick={async () => {
                        await authApi.revokePasskey(
                          String(credential.credential_id),
                        );
                        await refetch();
                      }}
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
      <Show when={activeTab() === "oidc"}>
        <section
          id="credential-panel-oidc"
          role="tabpanel"
          aria-labelledby="credential-tab-oidc"
          class="ui-card ui-stack-sm"
        >
          <h2 class="text-lg font-semibold">{t("securityPage.oidcTitle")}</h2>
          <p class="ui-muted">
            {t("securityPage.oidcDescription")}
          </p>
          <Show when={credentials()}>
            {(value) => (
              <For each={value().oidcProviders}>
                {(provider) => (
                  <button
                    type="button"
                    class="ui-button ui-button-secondary"
                    onClick={() => authApi.linkOidc(provider.provider_id)}
                  >
                    {t("securityPage.link", { issuer: provider.issuer })}
                  </button>
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
                      {t("securityPage.lastSeen")}{" "}
                      {String(session.last_seen_at ?? t("securityPage.never"))}
                      {session.revoked_at
                        ? ` · ${t("securityPage.revoked")}`
                        : ""}
                    </span>
                    <Show when={!session.revoked_at}>
                      <button
                        type="button"
                        class="ui-button ui-button-secondary"
                        onClick={async () => {
                          await authApi.revokeSession(
                            String(session.session_id),
                          );
                          await refetch();
                        }}
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
      <Show when={activeTab() === "totp"}>
        <section
          id="credential-panel-totp"
          role="tabpanel"
          aria-labelledby="credential-tab-totp"
          class="ui-card ui-stack-sm"
        >
          <h2 class="text-lg font-semibold">
            {t("securityPage.recoveryTotp")}
          </h2>
          <p class="ui-muted">
            {t("securityPage.recoveryDescription")}
          </p>
          <Show
            when={totp()}
            fallback={
              <button
                type="button"
                class="ui-button ui-button-primary"
                onClick={async () =>
                  setTotp(await authApi.startTotpEnrollment())}
              >
                {t("securityPage.enrollRecovery")}
              </button>
            }
          >
            {(enrollment) => (
              <div class="ui-stack-sm">
                <p>{t("securityPage.secretHelp")}</p>
                <code class="ui-card break-all">{enrollment().secret}</code>
                <label class="ui-stack-sm">
                  <span>{t("securityPage.currentCode")}</span>
                  <input
                    class="ui-input"
                    inputmode="numeric"
                    autocomplete="one-time-code"
                    value={totpCode()}
                    onInput={(event) => setTotpCode(event.currentTarget.value)}
                  />
                </label>
                <button
                  type="button"
                  class="ui-button ui-button-primary"
                  onClick={async () => {
                    await authApi.finishTotpEnrollment(totpCode());
                    setTotpConfigured(true);
                    setTotp(undefined);
                  }}
                >
                  {t("securityPage.confirmTotp")}
                </button>
              </div>
            )}
          </Show>
          <Show when={totpConfigured()}>
            <p class="ui-alert ui-alert-success">
              {t("securityPage.configured")}
            </p>
          </Show>
        </section>
      </Show>
      <Show when={activeTab() === "devices"}>
        <section
          id="credential-panel-devices"
          role="tabpanel"
          aria-labelledby="credential-tab-devices"
          class="ui-card ui-stack-sm"
        >
          <h2 class="text-lg font-semibold">{t("securityPage.devices")}</h2>
          <Show when={credentials()}>
            {(value) => (
              <For each={value().devices}>
                {(credential) => (
                  <div class="flex items-center justify-between">
                    <span>
                      {String(credential.device_name)} ·{" "}
                      {t("securityPage.lastUsed")} {String(
                        credential.last_used_at ?? t("securityPage.never"),
                      )}
                    </span>
                    <button
                      type="button"
                      class="ui-button ui-button-secondary"
                      onClick={async () => {
                        await authApi.revokeDevice(
                          String(credential.credential_id),
                        );
                        await refetch();
                      }}
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
    </>
  );
}
