import { A, useNavigate } from "@solidjs/router";
import { Show, type JSX } from "solid-js";
import { UiIcon } from "~/components/UiIcon";
import { AccountMenu } from "~/components/AccountMenu";
import { locale } from "~/lib/i18n";

const labels = {
  en: {
    home: "Home",
    forms: "Forms",
    search: "Search",
    settings: "Settings",
    spaces: "Spaces",
    about: "About",
    menu: "Menu",
    signIn: "Sign in",
  },
  ja: {
    home: "ホーム",
    forms: "フォーム",
    search: "検索",
    settings: "設定",
    spaces: "スペース",
    about: "Ugoiteについて",
    menu: "メニュー",
    signIn: "ログイン",
  },
} as const;

export function GlobalShell(
  props: {
    title: string;
    children: JSX.Element;
    active?: "spaces" | "about";
    authenticated?: boolean;
  },
) {
  const navigate = useNavigate();
  const copy = () => labels[locale() === "ja" ? "ja" : "en"];
  return (
    <main class="app workspaceApp">
      <div class="desktopSidebar">
        <aside class="sidebar">
          <A class="brand" href="/spaces">
            <span class="brandMark">U</span>
            <span>Ugoite</span>
          </A>
          <nav class="navGroup">
            <A class="navItem" href="/spaces">
              <UiIcon name="home" />
              <span>{copy().home}</span>
            </A>
            <A class="navItem" href="/spaces">
              <UiIcon name="forms" />
              <span>{copy().forms}</span>
            </A>
            <A class="navItem" href="/spaces">
              <UiIcon name="search" />
              <span>{copy().search}</span>
            </A>
            <A class="navItem" href="/spaces">
              <UiIcon name="settings" />
              <span>{copy().settings}</span>
            </A>
          </nav>
          <div class="sideFoot">
            <A
              class="navItem"
              classList={{ active: props.active === "spaces" }}
              href="/spaces"
            >
              <UiIcon name="spaces" />
              <span>{copy().spaces}</span>
            </A>
            <A
              class="navItem"
              classList={{ active: props.active === "about" }}
              href="/about"
            >
              <UiIcon name="about" />
              <span>{copy().about}</span>
            </A>
          </div>
        </aside>
      </div>
      <section class="main">
        <header class="topbar">
          <A
            class="btn iconBtn mobileMenu"
            href="/spaces"
            aria-label={copy().menu}
          >
            <UiIcon name="menu" />
          </A>
          <select class="spaceSelect" aria-label="Space">
            <option>Ugoite</option>
          </select>
          <div class="crumbTop">{props.title}</div>
          <Show
            when={props.authenticated !== false}
            fallback={<A class="btn" href="/login">{copy().signIn}</A>}
          >
            <AccountMenu
              settingsLink={(linkProps) => <A {...linkProps} />}
              onSignOut={() => navigate("/login", { replace: true })}
            />
          </Show>
        </header>
        <div class="content">{props.children}</div>
      </section>
      <nav class="bottomNav">
        <A class="active" href="/spaces">
          <UiIcon name="home" />
          <span>{copy().home}</span>
        </A>
        <A href="/spaces">
          <UiIcon name="forms" />
          <span>{copy().forms}</span>
        </A>
        <A href="/spaces">
          <UiIcon name="search" />
          <span>{copy().search}</span>
        </A>
        <A href="/spaces">
          <UiIcon name="settings" />
          <span>{copy().settings}</span>
        </A>
      </nav>
    </main>
  );
}
