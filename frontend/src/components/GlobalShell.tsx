import { A } from "@solidjs/router";
import { type JSX, Show } from "solid-js";
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
    authenticated?: boolean;
  },
) {
  const copy = () => labels[locale() === "ja" ? "ja" : "en"];
  return (
    <main class="app workspaceApp">
      <div class="desktopSidebar">
        <aside class="sidebar">
          <a class="brand" href="/spaces">
            <span class="brandMark">U</span>
            <span>Ugoite</span>
          </a>
          <nav class="navGroup">
            <a class="navItem" href="/spaces">
              <UiIcon name="home" />
              <span>{copy().home}</span>
            </a>
            <a class="navItem" href="/spaces">
              <UiIcon name="forms" />
              <span>{copy().forms}</span>
            </a>
            <a class="navItem" href="/spaces">
              <UiIcon name="search" />
              <span>{copy().search}</span>
            </a>
            <a class="navItem" href="/spaces">
              <UiIcon name="settings" />
              <span>{copy().settings}</span>
            </a>
          </nav>
          <div class="sideFoot">
            <A class="navItem" href="/spaces" end>
              <UiIcon name="spaces" />
              <span>{copy().spaces}</span>
            </A>
            <A class="navItem" href="/about" end>
              <UiIcon name="about" />
              <span>{copy().about}</span>
            </A>
          </div>
        </aside>
      </div>
      <section class="main">
        <header class="topbar">
          <a
            class="btn iconBtn mobileMenu"
            href="/spaces"
            aria-label={copy().menu}
          >
            <UiIcon name="menu" />
          </a>
          <select class="spaceSelect" aria-label="Space">
            <option>Ugoite</option>
          </select>
          <div class="crumbTop">{props.title}</div>
          <Show
            when={props.authenticated !== false}
            fallback={<a class="btn" href="/login">{copy().signIn}</a>}
          >
            <AccountMenu />
          </Show>
        </header>
        <div class="content">{props.children}</div>
      </section>
      <nav class="bottomNav">
        <a href="/spaces">
          <UiIcon name="home" />
          <span>{copy().home}</span>
        </a>
        <a href="/spaces">
          <UiIcon name="forms" />
          <span>{copy().forms}</span>
        </a>
        <a href="/spaces">
          <UiIcon name="search" />
          <span>{copy().search}</span>
        </a>
        <a href="/spaces">
          <UiIcon name="settings" />
          <span>{copy().settings}</span>
        </a>
      </nav>
    </main>
  );
}
