import { createSignal, Show } from "solid-js";
import { authApi } from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";

export function AccountMenu(props: { settingsHref?: string } = {}) {
  const [open, setOpen] = createSignal(false);

  const signOut = async () => {
    await authApi.clearSession();
    if (typeof window !== "undefined") window.location.assign("/login");
  };

  return (
    <div class="accountMenu">
      <button
        class="avatar"
        type="button"
        aria-label={t("account.title")}
        aria-expanded={open()}
        aria-haspopup="menu"
        onClick={() => setOpen((value) => !value)}
      >
        S
      </button>
      <Show when={open()}>
        <div class="accountMenuPanel" role="menu">
          <div class="accountMenuTitle">{t("account.title")}</div>
          <a
            href={props.settingsHref ?? "/settings/security"}
            role="menuitem"
            onClick={() => setOpen(false)}
          >
            {t("account.settings")}
          </a>
          <button
            type="button"
            role="menuitem"
            onClick={() => void signOut()}
          >
            {t("account.signOut")}
          </button>
        </div>
      </Show>
    </div>
  );
}
