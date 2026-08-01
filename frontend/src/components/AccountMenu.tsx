import { A, useNavigate, useParams } from "@solidjs/router";
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
  const navigate = useNavigate();
  const params = useParams<{ space_id?: string }>();
  const [open, setOpen] = createSignal(false);
  const copy = () => labels[locale() === "ja" ? "ja" : "en"];
  const settingsHref = () =>
    params.space_id
      ? `/spaces/${params.space_id}/settings?section=credentials`
      : "/settings/security";

  const signOut = async () => {
    await authApi.clearSession();
    navigate("/login", { replace: true });
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
          <A
            href={settingsHref()}
            role="menuitem"
            onClick={() => setOpen(false)}
          >
            {copy().settings}
          </A>
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
