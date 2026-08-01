import { A, useNavigate } from "@solidjs/router";
import type { JSX } from "solid-js";
import { createMemo, createSignal, For, onMount, Show } from "solid-js";
import { locale } from "~/lib/i18n";
import { loadingState } from "~/lib/loading";
import { UiIcon, type UiIconName } from "~/components/UiIcon";
import { AccountMenu } from "~/components/AccountMenu";
import { createSpaceStore } from "~/lib/space-store";

export type SpaceNavigation = "home" | "forms" | "search" | "settings";

interface SpaceShellProps {
  spaceId: string;
  activeNavigation: SpaceNavigation;
  title?: string;
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
  const navigate = useNavigate();
  const [drawerOpen, setDrawerOpen] = createSignal(false);
  onMount(() => {
    void spaceStore.loadSpaces().catch(() => undefined);
  });
  const copy = () => labels[locale() === "ja" ? "ja" : "en"];
  const active = () => props.activeNavigation;
  const crumb = createMemo(() => props.title ?? copy()[active()]);
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
      aria-label="Space navigation"
    >
      {navItems.map((item) => (
        <A
          href={`/spaces/${props.spaceId}/${item.path}`}
          class={mobile ? "" : "navItem"}
          activeClass=""
          inactiveClass=""
          classList={{ active: active() === item.id }}
          aria-current={active() === item.id ? "page" : undefined}
          onClick={() => setDrawerOpen(false)}
        >
          <UiIcon name={item.icon} />
          <span>{copy()[item.id]}</span>
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
        <A class="brand" href={`/spaces/${props.spaceId}/dashboard`}>
          <span class="brandMark">U</span>
          <span>Ugoite</span>
        </A>
        <label class="sidebarSpaceSelect">
          <span class="ui-sr-only">Space</span>
          <select
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
                <option value={space.id} selected={space.id === props.spaceId}>
                  {space.name || space.id}
                </option>
              )}
            </For>
          </select>
        </label>
        {navigation()}
        <div class="sideFoot">
          <A class="navItem" href="/spaces">
            <UiIcon name="spaces" />
            <span>{copy().spaces}</span>
          </A>
          <A class="navItem" href="/about">
            <UiIcon name="about" />
            <span>{copy().about}</span>
          </A>
        </div>
      </aside>
    );
  }
}
