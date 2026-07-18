import type { JSX } from "solid-js";
import { createMemo, createSignal, For, onMount, Show } from "solid-js";
import { locale } from "~/lib/i18n";
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

const labels = {
  en: {
    home: "Home",
    forms: "Forms",
    search: "Search",
    settings: "Settings",
    spaces: "Spaces",
    about: "About",
    menu: "Menu",
    account: "Account",
  },
  ja: {
    home: "ホーム",
    forms: "フォーム",
    search: "検索",
    settings: "設定",
    spaces: "スペース",
    about: "Ugoiteについて",
    menu: "メニュー",
    account: "アカウント",
  },
} as const;

export function SpaceShell(props: SpaceShellProps) {
  const spaceStore = createSpaceStore();
  const [drawerOpen, setDrawerOpen] = createSignal(false);
  onMount(() => {
    void spaceStore.loadSpaces().catch(() => undefined);
  });
  const copy = () => labels[locale() === "ja" ? "ja" : "en"];
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
  const crumb = createMemo(() => props.title ?? copy()[active()]);
  const activePath = createMemo(() =>
    navItems.find((item) => item.id === active())?.path ?? "dashboard"
  );

  const switchSpace = (spaceId: string) => {
    if (!spaceId || spaceId === props.spaceId) return;
    spaceStore.selectSpace(spaceId);
    if (typeof window !== "undefined") {
      window.location.assign(`/spaces/${spaceId}/${activePath()}`);
    }
  };

  const navigation = (mobile = false) => (
    <nav
      class={mobile ? "bottomNav" : "navGroup"}
      aria-label="Space navigation"
    >
      {navItems.map((item) => (
        <a
          href={`/spaces/${props.spaceId}/${item.path}`}
          class={mobile ? "" : "navItem"}
          classList={{ active: active() === item.id }}
          aria-current={active() === item.id ? "page" : undefined}
          onClick={() => setDrawerOpen(false)}
        >
          <UiIcon name={item.icon} />
          <span>{copy()[item.id]}</span>
        </a>
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
          aria-label="Close menu"
          onClick={() => setDrawerOpen(false)}
        />
        <div class="mobileDrawer">{sidebar()}</div>
      </Show>
      <section class="main">
        <header class="topbar">
          <button
            class="btn iconBtn mobileMenu"
            type="button"
            aria-label={copy().menu}
            onClick={() => setDrawerOpen(true)}
          >
            <UiIcon name="menu" />
          </button>
          <select
            class="spaceSelect"
            aria-label="Space"
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
                <option
                  value={space.id}
                  selected={space.id === props.spaceId}
                >
                  {space.name || space.id}
                </option>
              )}
            </For>
          </select>
          <div class="crumbTop">{crumb()}</div>
          <AccountMenu />
        </header>
        <div class="content">{props.children}</div>
      </section>
      {navigation(true)}
    </main>
  );

  function sidebar() {
    return (
      <aside class="sidebar">
        <a class="brand" href={`/spaces/${props.spaceId}/dashboard`}>
          <span class="brandMark">U</span>
          <span>Ugoite</span>
        </a>
        {navigation()}
        <div class="sideFoot">
          <a class="navItem" href="/spaces">
            <UiIcon name="spaces" />
            <span>{copy().spaces}</span>
          </a>
          <a class="navItem" href="/about">
            <UiIcon name="about" />
            <span>{copy().about}</span>
          </a>
        </div>
      </aside>
    );
  }
}
