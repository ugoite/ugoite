import { A, useNavigate } from "@solidjs/router";
import type { JSX } from "solid-js";
import { createMemo, createSignal, For, onMount, Show } from "solid-js";
import { t, type TranslationKey } from "~/lib/i18n";
import { loadingState } from "~/lib/loading";
import { UiIcon, type UiIconName } from "~/components/UiIcon";
import { AccountMenu } from "~/components/AccountMenu";
import { createSpaceStore } from "~/lib/space-store";

export type SpaceTopTab = "dashboard" | "search";
export type SpaceBottomTab = "object" | "grid";
export type SpaceNavigation = "home" | "forms" | "search" | "settings";

interface SpaceShellProps {
  spaceId: string;
  activeTopTab?: SpaceTopTab;
  activeBottomTab?: SpaceBottomTab;
  activeNavigation?: SpaceNavigation;
  title?: string;
  showBottomTabs?: boolean;
  bottomTabHrefSuffix?: string;
  children: JSX.Element;
}

const navItems: Array<{ id: SpaceNavigation; icon: UiIconName; path: string }> =
  [
    { id: "home", icon: "home", path: "dashboard" },
    { id: "forms", icon: "forms", path: "forms" },
    { id: "search", icon: "search", path: "search" },
    { id: "settings", icon: "settings", path: "settings" },
  ];

const navigationLabels: Record<SpaceNavigation, TranslationKey> = {
  home: "nav.home",
  forms: "spaceShell.bottom.grid",
  search: "spaceShell.top.search",
  settings: "spaceShell.nav.settings",
};

export function SpaceShell(props: SpaceShellProps) {
  const spaceStore = createSpaceStore();
  const navigate = useNavigate();
  const [drawerOpen, setDrawerOpen] = createSignal(false);
  onMount(() => {
    void spaceStore.loadSpaces().catch(() => undefined);
  });
  const active = createMemo<SpaceNavigation>(() => {
    if (props.activeNavigation) return props.activeNavigation;
    if (props.activeTopTab === "dashboard") return "home";
    if (props.activeTopTab === "search") return "search";
    if (props.activeBottomTab === "grid") return "forms";
    const pathname = typeof window === "undefined"
      ? ""
      : window.location.pathname;
    if (pathname.includes("/settings")) return "settings";
    if (
      pathname.includes("/search") || pathname.includes("/sql") ||
      pathname.includes("/queries")
    ) return "search";
    if (
      pathname.includes("/forms") || pathname.includes("/entries") ||
      pathname.includes("/assets")
    ) return "forms";
    return "home";
  });
  const crumb = createMemo(() => props.title ?? t(navigationLabels[active()]));
  const activePath = createMemo(() =>
    navItems.find((item) => item.id === active())?.path ?? "dashboard"
  );

  const switchSpace = (spaceId: string) => {
    if (!spaceId || spaceId === props.spaceId) return;
    spaceStore.selectSpace(spaceId);
    navigate(`/spaces/${spaceId}/${activePath()}`);
  };

  const navigation = (mobile = false) => (
    <nav
      class={mobile ? "bottomNav" : "navGroup"}
      aria-label={t("spaceShell.navigation")}
    >
      {navItems.map((item) => (
        <A
          href={`/spaces/${props.spaceId}/${item.path}`}
          class={mobile ? "" : "navItem"}
          classList={{ active: active() === item.id }}
          aria-current={active() === item.id ? "page" : undefined}
          onClick={() => setDrawerOpen(false)}
        >
          <UiIcon name={item.icon} />
          <span>{t(navigationLabels[item.id])}</span>
        </A>
      ))}
    </nav>
  );

  return (
    <main class="app workspaceApp">
      <Show when={loadingState.isLoading()}>
        <div class="loadingBar" />
      </Show>
      <div class="desktopSidebar">{sidebar()}</div>
      <Show when={drawerOpen()}>
        <button
          type="button"
          class="drawerBackdrop"
          aria-label={t("spaceShell.closeMenu")}
          onClick={() => setDrawerOpen(false)}
        />
        <div class="mobileDrawer">{sidebar()}</div>
      </Show>
      <section class="main">
        <header class="topbar">
          <button
            class="btn iconBtn mobileMenu"
            type="button"
            aria-label={t("common.menu")}
            onClick={() => setDrawerOpen(true)}
          >
            <UiIcon name="menu" />
          </button>
          <div class="crumbTop">{crumb()}</div>
          <AccountMenu
            settingsHref={`/spaces/${props.spaceId}/settings?section=credentials`}
          />
        </header>
        <div class="content">{props.children}</div>
      </section>
      {navigation(true)}
    </main>
  );

  function sidebar() {
    return (
      <aside class="sidebar">
        <A class="brand" href={`/spaces/${props.spaceId}/dashboard`}>
          <span class="brandMark">U</span>
          <span>Ugoite</span>
        </A>
        <label class="sidebarSpaceSelect">
          <span class="ui-sr-only">{t("common.space")}</span>
          <select
            aria-label={t("common.space")}
            value={props.spaceId}
            onChange={(event) => switchSpace(event.currentTarget.value)}
          >
            <Show
              when={!spaceStore.spaces().some((space) =>
                space.id === props.spaceId
              )}
            >
              <option value={props.spaceId}>{props.spaceId}</option>
            </Show>
            <For each={spaceStore.spaces()}>
              {(space) => (
                <option value={space.id} selected={space.id === props.spaceId}>
                  {space.name || space.id}
                </option>
              )}
            </For>
          </select>
        </label>
        {navigation()}
        <div class="sideFoot">
          <A class="navItem" href="/spaces" end>
            <UiIcon name="spaces" />
            <span>{t("nav.spaces")}</span>
          </A>
          <A class="navItem" href="/about" end>
            <UiIcon name="about" />
            <span>{t("nav.about")}</span>
          </A>
        </div>
      </aside>
    );
  }
}
