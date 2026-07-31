import { type JSX, Show } from "solid-js";
import { UiIcon } from "~/components/UiIcon";
import { AccountMenu } from "~/components/AccountMenu";
import { t } from "~/lib/i18n";

export function GlobalShell(
  props: {
    title: string;
    children: JSX.Element;
    active?: "spaces" | "about";
    authenticated?: boolean;
  },
) {
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
              <span>{t("nav.home")}</span>
            </a>
            <a class="navItem" href="/spaces">
              <UiIcon name="forms" />
              <span>{t("spaceShell.bottom.grid")}</span>
            </a>
            <a class="navItem" href="/spaces">
              <UiIcon name="search" />
              <span>{t("spaceShell.top.search")}</span>
            </a>
            <a class="navItem" href="/spaces">
              <UiIcon name="settings" />
              <span>{t("globalShell.settings")}</span>
            </a>
          </nav>
          <div class="sideFoot">
            <a
              class="navItem"
              classList={{ active: props.active === "spaces" }}
              href="/spaces"
            >
              <UiIcon name="spaces" />
              <span>{t("nav.spaces")}</span>
            </a>
            <a
              class="navItem"
              classList={{ active: props.active === "about" }}
              href="/about"
            >
              <UiIcon name="about" />
              <span>{t("nav.about")}</span>
            </a>
          </div>
        </aside>
      </div>
      <section class="main">
        <header class="topbar">
          <a
            class="btn iconBtn mobileMenu"
            href="/spaces"
            aria-label={t("common.menu")}
          >
            <UiIcon name="menu" />
          </a>
          <select class="spaceSelect" aria-label={t("common.space")}>
            <option>Ugoite</option>
          </select>
          <div class="crumbTop">{props.title}</div>
          <Show
            when={props.authenticated !== false}
            fallback={
              <a class="btn" href="/login">{t("globalShell.signIn")}</a>
            }
          >
            <AccountMenu />
          </Show>
        </header>
        <div class="content">{props.children}</div>
      </section>
      <nav class="bottomNav">
        <a class="active" href="/spaces">
          <UiIcon name="home" />
          <span>{t("nav.home")}</span>
        </a>
        <a href="/spaces">
          <UiIcon name="forms" />
          <span>{t("spaceShell.bottom.grid")}</span>
        </a>
        <a href="/spaces">
          <UiIcon name="search" />
          <span>{t("spaceShell.top.search")}</span>
        </a>
        <a href="/spaces">
          <UiIcon name="settings" />
          <span>{t("globalShell.settings")}</span>
        </a>
      </nav>
    </main>
  );
}
