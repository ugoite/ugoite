import { createResource, createSignal, For, Show } from "solid-js";
import { authApi } from "~/lib/auth-api";

export default function SecuritySettingsRoute() {
  const [totp, setTotp] = createSignal<{
    secret: string;
    otpauth_uri: string;
  }>();
  const [totpCode, setTotpCode] = createSignal("");
  const [totpConfigured, setTotpConfigured] = createSignal(false);
  const [credentials, { refetch }] = createResource(async () => ({
    passkeys: await authApi.listPasskeys(),
    sessions: await authApi.listSessions(),
    devices: await authApi.listDevices(),
    oidcProviders: await authApi.listOidcProviders(),
  }));
  return (
    <main class="ui-page mx-auto max-w-3xl ui-stack">
      <h1 class="ui-page-title">Credentials</h1>
      <section class="ui-card ui-stack-sm">
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
      <section class="ui-card ui-stack-sm">
        <h2 class="text-lg font-semibold">OIDC login methods</h2>
        <p class="ui-muted">
          Link an optional provider to this account. Linking requires a recent
          Passkey authentication and uses the provider subject, never email.
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
      <section class="ui-card ui-stack-sm">
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
                        await authApi.revokeSession(String(session.session_id));
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
      <section class="ui-card ui-stack-sm">
        <h2 class="text-lg font-semibold">Recovery TOTP</h2>
        <p class="ui-muted">
          TOTP is usable only together with one of your one-use recovery codes.
          Enrollment requires a recent Passkey authentication.
        </p>
        <Show
          when={totp()}
          fallback={
            <button
              type="button"
              class="ui-button ui-button-primary"
              onClick={async () => setTotp(await authApi.startTotpEnrollment())}
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
                Confirm TOTP
              </button>
            </div>
          )}
        </Show>
        <Show when={totpConfigured()}>
          <p class="ui-alert ui-alert-success">Recovery TOTP configured.</p>
        </Show>
      </section>
      <section class="ui-card ui-stack-sm">
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
    </main>
  );
}
