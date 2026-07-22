import { useSearchParams } from "@solidjs/router";
import { createEffect, createResource, createSignal, For, Show } from "solid-js";
import { authApi } from "~/lib/auth-api";
import { GlobalShell } from "~/components/GlobalShell";

const credentialTabs = [
  ["passkeys", "Passkeys"],
  ["oidc", "OIDC"],
  ["sessions", "Sessions"],
  ["totp", "Recovery TOTP"],
  ["devices", "CLI / MCP"],
] as const;

type CredentialTab = typeof credentialTabs[number][0];

const credentialTabFromSearch = (value: unknown): CredentialTab =>
  credentialTabs.some(([id]) => id === value)
    ? value as CredentialTab
    : "passkeys";

export default function SecuritySettingsRoute() {
  return (
    <GlobalShell title="Settings / Credentials">
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">Ugoite</div>
          <h1>Settings</h1>
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
        <h2>Credentials</h2>
        <div class="tabs" role="tablist" aria-label="Credential settings">
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
                {label}
              </button>
            )}
          </For>
        </div>
        <Show when={activeTab() === "passkeys"}>
          <section
            id="credential-panel-passkeys"
            role="tabpanel"
            aria-labelledby="credential-tab-passkeys"
            class="ui-card ui-stack-sm"
          >
            <div class="flex items-center justify-between">
              <h2 class="text-lg font-semibold">Passkeys</h2>
              <button
                type="button"
                class="ui-button ui-button-primary"
                onClick={async () => {
                  await authApi.addPasskey();
                  await refetch();
                }}
              >
                Add Passkey
              </button>
            </div>
            <Show when={credentials()}>
              {(value) => (
                <For each={value().passkeys}>
                  {(credential) => (
                    <div class="flex items-center justify-between">
                      <span>
                        {String(credential.credential_id)} · last used{" "}
                        {String(credential.last_used_at ?? "never")}
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
                        Revoke
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
            <h2 class="text-lg font-semibold">OIDC login methods</h2>
            <p class="ui-muted">
              Link an optional provider to this account. Linking requires a
              recent Passkey authentication and uses the provider subject, never
              email.
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
                      Link {provider.issuer}
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
            <h2 class="text-lg font-semibold">Browser sessions</h2>
            <Show when={credentials()}>
              {(value) => (
                <For each={value().sessions}>
                  {(session) => (
                    <div class="flex items-center justify-between">
                      <span>
                        {String(session.session_id)} · last seen{" "}
                        {String(session.last_seen_at ?? "never")}
                        {session.revoked_at ? " · revoked" : ""}
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
                          Revoke
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
            <h2 class="text-lg font-semibold">Recovery TOTP</h2>
            <p class="ui-muted">
              TOTP is usable only together with one of your one-use recovery
              codes. Enrollment requires a recent Passkey authentication.
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
                  Enroll recovery TOTP
                </button>
              }
            >
              {(enrollment) => (
                <div class="ui-stack-sm">
                  <p>Enter this secret in your authenticator:</p>
                  <code class="ui-card break-all">{enrollment().secret}</code>
                  <label class="ui-stack-sm">
                    <span>Current six-digit code</span>
                    <input
                      class="ui-input"
                      inputmode="numeric"
                      autocomplete="one-time-code"
                      value={totpCode()}
                      onInput={(event) =>
                        setTotpCode(event.currentTarget.value)}
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
                    Confirm TOTP
                  </button>
                </div>
              )}
            </Show>
            <Show when={totpConfigured()}>
              <p class="ui-alert ui-alert-success">Recovery TOTP configured.</p>
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
            <h2 class="text-lg font-semibold">CLI and MCP devices</h2>
            <Show when={credentials()}>
              {(value) => (
                <For each={value().devices}>
                  {(credential) => (
                    <div class="flex items-center justify-between">
                      <span>
                        {String(credential.device_name)} · last used{" "}
                        {String(credential.last_used_at ?? "never")}
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
                        Revoke
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
