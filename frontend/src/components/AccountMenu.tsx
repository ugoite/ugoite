import { createSignal, type JSX, Show } from "solid-js";
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

type AccountMenuLinkProps = {
  href: string;
  role: "menuitem";
  onClick: () => void;
  children: JSX.Element;
};

type AccountMenuProps = {
  settingsHref?: string;
  settingsLink?: (props: AccountMenuLinkProps) => JSX.Element;
  onSignOut?: () => void | Promise<void>;
};

export function AccountMenu(props: AccountMenuProps = {}) {
  const [open, setOpen] = createSignal(false);
  const copy = () => labels[locale() === "ja" ? "ja" : "en"];

  const signOut = async () => {
    await authApi.clearSession();
    if (props.onSignOut) {
      await props.onSignOut();
      return;
    }
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
          {props.settingsLink
            ? props.settingsLink({
              href: props.settingsHref ?? "/settings/security",
              role: "menuitem",
              onClick: () => setOpen(false),
              children: copy().settings,
            })
            : (
              <a
                href={props.settingsHref ?? "/settings/security"}
                role="menuitem"
                onClick={() => setOpen(false)}
              >
                {copy().settings}
              </a>
            )}
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
