import type { JSX } from "solid-js";
import { UiIcon } from "~/components/UiIcon";
import { locale } from "~/lib/i18n";
import { authApi } from "~/lib/ugoite-client";

const labels = {
  en: {
    home: "Home",
    forms: "Forms",
    search: "Search",
    settings: "Settings",
    spaces: "Spaces",
    about: "About",
    menu: "Menu",
    signOut: "Sign out",
  },
  ja: {
    home: "ホーム",
    forms: "フォーム",
    search: "検索",
    settings: "設定",
    spaces: "スペース",
    about: "Ugoiteについて",
    menu: "メニュー",
    signOut: "ログアウト",
  },
} as const;

export function GlobalShell(
  props: { title: string; children: JSX.Element; active?: "spaces" | "about" },
) {
  const copy = () => labels[locale() === "ja" ? "ja" : "en"];
  const signOut = async () => {
    await authApi.clearSession();
    if (typeof window !== "undefined") window.location.assign("/login");
  };
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
            <a
              class="navItem"
              classList={{ active: props.active === "spaces" }}
              href="/spaces"
            >
              <UiIcon name="spaces" />
              <span>{copy().spaces}</span>
            </a>
            <a
              class="navItem"
              classList={{ active: props.active === "about" }}
              href="/about"
            >
              <UiIcon name="about" />
              <span>{copy().about}</span>
            </a>
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
          <button
            class="avatar"
            type="button"
            onClick={() => void signOut()}
            aria-label={copy().signOut}
          >
            S
          </button>
        </header>
        <div class="content">{props.children}</div>
      </section>
      <nav class="bottomNav">
        <a class="active" href="/spaces">
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
