import { createSignal, Show } from "solid-js";
import { authApi } from "~/lib/ugoite-client";
import { locale } from "~/lib/i18n";

const labels = {
  en: {
    account: "Account",
    settings: "Account settings",
    signOut: "Sign out",
  },
  ja: {
    account: "アカウント",
    settings: "アカウント設定",
    signOut: "ログアウト",
  },
} as const;

export function AccountMenu() {
  const [open, setOpen] = createSignal(false);
  const copy = () => labels[locale() === "ja" ? "ja" : "en"];

  const signOut = async () => {
    await authApi.clearSession();
    if (typeof window !== "undefined") window.location.assign("/login");
  };

  return (
    <div class="accountMenu">
      <button
        class="avatar"
        type="button"
        aria-label={copy().account}
        aria-expanded={open()}
        aria-haspopup="menu"
        onClick={() => setOpen((value) => !value)}
      >
        S
      </button>
      <Show when={open()}>
        <div class="accountMenuPanel" role="menu">
          <div class="accountMenuTitle">{copy().account}</div>
          <a
            href="/settings/security"
            role="menuitem"
            onClick={() => setOpen(false)}
          >
            {copy().settings}
          </a>
          <button
            type="button"
            role="menuitem"
            onClick={() => void signOut()}
          >
            {copy().signOut}
          </button>
        </div>
      </Show>
    </div>
  );
}
