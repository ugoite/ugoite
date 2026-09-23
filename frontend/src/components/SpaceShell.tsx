import { A, useNavigate } from "@solidjs/router";
import type { JSX } from "solid-js";
import { createMemo, createSignal, For, onMount, Show } from "solid-js";
import { t, type TranslationKey } from "~/lib/i18n";
import { UiIcon, type UiIconName } from "~/components/UiIcon";
import { AccountMenu } from "~/components/AccountMenu";
import { KonasePanel } from "~/components/konase/KonasePanel";
import { createSpaceStore } from "~/lib/space-store";
import { spaceUid } from "~/lib/space-list";
import { spacePath, spaceSettingsPath } from "~/lib/space-path";

// Form-first navigation: Forms is the entry point to a Form's Entries.
// Desktop: Home / Knowledge(Assets, Forms) / Explore(Search)
//   / Recovery(History) / Settings.
// Mobile bottom nav: Home, Forms, Search, History, More — More contains
// Assets, Settings. Both surfaces expose the same six destinations;
// Entries is not an independent destination.
export type SpaceNavigation =
  | "home"
  | "assets"
  | "forms"
  | "search"
  | "history"
  | "settings";

interface SpaceShellProps {
  spaceId: string;
  activeNavigation: SpaceNavigation;
  title?: string;
  showBottomTabs?: boolean;
  bottomTabHrefSuffix?: string;
  children: JSX.Element;
}

export interface SpaceNavItem {
  id: SpaceNavigation;
  icon: UiIconName;
  path: string;
  labelKey: TranslationKey;
}

export const SPACE_NAV_ITEMS: SpaceNavItem[] = [
  {
    id: "home",
    icon: "home",
    path: "dashboard",
    labelKey: "spaceShell.nav.home",
  },
  {
    id: "assets",
    icon: "asset",
    path: "assets",
    labelKey: "spaceShell.nav.assets",
  },
  {
    id: "forms",
    icon: "forms",
    path: "forms",
    labelKey: "spaceShell.nav.forms",
  },
  {
    id: "search",
    icon: "search",
    path: "search",
    labelKey: "spaceShell.nav.search",
  },
  {
    id: "history",
    icon: "history",
    path: "history",
    labelKey: "spaceShell.nav.history",
  },
  {
    id: "settings",
    icon: "settings",
    path: "settings",
    labelKey: "spaceShell.nav.settings",
  },
];

export const MOBILE_PRIMARY_NAV: SpaceNavigation[] = [
  "home",
  "forms",
  "search",
  "history",
];

export const MOBILE_MORE_NAV: SpaceNavigation[] = [
  "assets",
  "settings",
];

export interface SpaceNavGroup {
  labelKey: TranslationKey | null;
  items: SpaceNavigation[];
}

export const DESKTOP_NAV_GROUPS: SpaceNavGroup[] = [
  { labelKey: null, items: ["home"] },
  {
    labelKey: "spaceShell.nav.knowledge",
    items: ["assets", "forms"],
  },
  { labelKey: "spaceShell.nav.explore", items: ["search"] },
  { labelKey: "spaceShell.nav.recovery", items: ["history"] },
  { labelKey: null, items: ["settings"] },
];

const navItemById = new Map<SpaceNavigation, SpaceNavItem>(
  SPACE_NAV_ITEMS.map((item) => [item.id, item]),
);

export function spaceNavItem(id: SpaceNavigation): SpaceNavItem {
  const item = navItemById.get(id);
  if (!item) throw new Error(`Unknown space navigation: ${id}`);
  return item;
}

export function inferSpaceNavigation(pathname: string): SpaceNavigation {
  if (pathname.includes("/settings")) return "settings";
  if (pathname.includes("/history")) return "history";
  if (
    pathname.includes("/search") || pathname.includes("/sql") ||
    pathname.includes("/queries")
  ) return "search";
  if (pathname.includes("/assets")) return "assets";
  if (pathname.includes("/forms")) return "forms";
  // Entry detail, creation, and history routes live under Forms: Forms is
  // the entry point to a Form's Entries.
  if (pathname.includes("/entries")) return "forms";
  return "home";
}

export function SpaceShell(props: SpaceShellProps) {
  const spaceStore = createSpaceStore();
  const navigate = useNavigate();
  const [drawerOpen, setDrawerOpen] = createSignal(false);
  const [konaseOpen, setKonaseOpen] = createSignal(false);
  onMount(() => {
    void spaceStore.loadSpaces().catch(() => undefined);
  });
  const active = createMemo<SpaceNavigation>(() => {
    if (props.activeNavigation) return props.activeNavigation;
    const pathname = typeof window === "undefined"
      ? ""
      : window.location.pathname;
    return inferSpaceNavigation(pathname);
  });
  const activePath = createMemo(() =>
    navItemById.get(active())?.path ?? "dashboard"
  );
  const activeLabelKey = createMemo<TranslationKey>(() =>
    navItemById.get(active())?.labelKey ?? "spaceShell.nav.home"
  );
  const crumb = createMemo(() => props.title ?? t(activeLabelKey()));

  const switchSpace = (spaceId: string) => {
    if (!spaceId || spaceId === props.spaceId) return;
    spaceStore.selectSpace(spaceId);
    navigate(spacePath(spaceId, activePath()));
  };

  const navLink = (id: SpaceNavigation, mobile = false) => {
    const item = spaceNavItem(id);
    return (
      <A
        href={spacePath(props.spaceId, item.path)}
        class={mobile ? "" : "navItem"}
        classList={{ active: active() === item.id }}
        aria-current={active() === item.id ? "page" : undefined}
        onClick={() => setDrawerOpen(false)}
      >
        <UiIcon name={item.icon} />
        <span>{t(item.labelKey)}</span>
      </A>
    );
  };

  const desktopNavigation = () => (
    <nav class="navGroup" aria-label={t("spaceShell.navigation")}>
      <For each={DESKTOP_NAV_GROUPS}>
        {(group) => (
          <>
            <Show when={group.labelKey}>
              <div class="navGroupLabel" aria-hidden="true">
                {t(group.labelKey as TranslationKey)}
              </div>
            </Show>
            <For each={group.items}>
              {(id) => navLink(id)}
            </For>
          </>
        )}
      </For>
    </nav>
  );

  const mobileNavigation = () => (
    <nav
      class="bottomNav"
      aria-label={t("spaceShell.navigation")}
    >
      <For each={MOBILE_PRIMARY_NAV}>
        {(id) => navLink(id, true)}
      </For>
      <details class="moreMenu">
        <summary aria-label={t("spaceShell.nav.more")}>
          <UiIcon name="menu" />
          <span>{t("spaceShell.nav.more")}</span>
        </summary>
        <div class="moreMenuItems">
          <For each={MOBILE_MORE_NAV}>
            {(id) => navLink(id, true)}
          </For>
        </div>
      </details>
    </nav>
  );

  return (
    <main class="app workspaceApp">
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
          <div class="topbarTools">
            <div class="crumbTop ui-sr-only">{crumb()}</div>
            <button
              class="pill iconpill"
              type="button"
              aria-label={t("konase.title")}
              aria-expanded={konaseOpen()}
              onClick={() => setKonaseOpen((open) => !open)}
            >
              <span class="assistantDot" aria-hidden="true" />
              <span class="ui-sr-only">{t("konase.title")}</span>
            </button>
            <AccountMenu
              settingsHref={spaceSettingsPath(
                props.spaceId,
                "?section=credentials",
              )}
            />
          </div>
        </header>
        <div class="konasePopover" hidden={!konaseOpen()}>
          <KonasePanel spaceId={props.spaceId} />
        </div>
        <div class="content">{props.children}</div>
      </section>
      {mobileNavigation()}
    </main>
  );

  function sidebar() {
    return (
      <aside class="sidebar">
        <A class="brand" href={spacePath(props.spaceId, "dashboard")}>
          <img
            class="brandMark"
            src="/brand/ugoite-mark.svg"
            alt=""
            aria-hidden="true"
          />
          <span>Ugoite</span>
        </A>
        <label class="sidebarSpaceSelect">
          <span class="ui-sr-only">{t("common.space")}</span>
          <span class="spaceIndicator" aria-hidden="true" />
          <select
            aria-label={t("common.space")}
            value={props.spaceId}
            onChange={(event) => switchSpace(event.currentTarget.value)}
          >
            <Show
              when={!spaceStore.spaces().some((space) =>
                spaceUid(space) === props.spaceId
              )}
            >
              <option value={props.spaceId}>{props.spaceId}</option>
            </Show>
            <For each={spaceStore.spaces()}>
              {(space) => (
                <option
                  value={spaceUid(space)}
                  selected={spaceUid(space) === props.spaceId}
                >
                  {space.name || space.slug || spaceUid(space)}
                </option>
              )}
            </For>
          </select>
        </label>
        {desktopNavigation()}
        <div class="sideFoot">
          <A class="navItem" href="/spaces" end>
            <UiIcon name="spaces" />
            <span>{t("nav.spaces")}</span>
          </A>
        </div>
      </aside>
    );
  }
}
