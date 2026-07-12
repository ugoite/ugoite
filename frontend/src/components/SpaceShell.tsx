import type { JSX } from "solid-js";
import { Show } from "solid-js";
import { loadingState } from "~/lib/loading";

export type SpaceTopTab = "dashboard" | "search";
export type SpaceBottomTab = "object" | "grid";
type SpaceNavigation = "home" | "forms" | "search" | "settings";

interface SpaceShellProps {
  spaceId: string;
  activeTopTab?: SpaceTopTab;
  activeBottomTab?: SpaceBottomTab;
  activeNavigation?: SpaceNavigation;
  /** @deprecated Navigation is now persistent; retained for route compatibility. */
  showBottomTabs?: boolean;
  /** @deprecated Form context is represented by the Forms workspace. */
  bottomTabHrefSuffix?: string;
  children: JSX.Element;
}

const navigation = [
  { id: "home", label: "Home", icon: "⌂", path: "dashboard" },
  { id: "forms", label: "Forms", icon: "▤", path: "forms" },
  { id: "search", label: "Search", icon: "⌕", path: "search" },
  { id: "settings", label: "Settings", icon: "⚙", path: "settings" },
] as const;

export function SpaceShell(props: SpaceShellProps) {
  const active = (): SpaceNavigation => props.activeNavigation ??
    (props.activeTopTab === "dashboard"
      ? "home"
      : props.activeTopTab === "search"
      ? "search"
      : props.activeBottomTab === "grid"
      ? "forms"
      : "forms");

  return (
    <main class="ui-shell ui-app-shell">
      <Show when={loadingState.isLoading()}>
        <div class="fixed top-0 left-0 right-0 z-[60] pointer-events-none">
          <div class="ui-loading-bar" />
        </div>
      </Show>

      <aside class="ui-global-sidebar" aria-label="Space navigation">
        <a class="ui-brand" href={`/spaces/${props.spaceId}/dashboard`}>
          <span class="ui-brand-mark">U</span><span>Ugoite</span>
        </a>
        <nav class="ui-global-nav">
          {navigation.map((item) => (
            <a
              href={`/spaces/${props.spaceId}/${item.path}`}
              class="ui-global-nav-item"
              classList={{ "ui-global-nav-item-active": active() === item.id }}
              aria-current={active() === item.id ? "page" : undefined}
            >
              <span aria-hidden="true" class="ui-nav-icon">{item.icon}</span>
              {item.label}
            </a>
          ))}
        </nav>
        <a class="ui-global-sidebar-footer" href="/spaces">Switch space</a>
      </aside>

      <section class="ui-app-main">
        <header class="ui-workspace-bar">
          <a href="/spaces" class="ui-space-switcher">{props.spaceId}</a>
          <span class="ui-workspace-hint">Operator-owned space</span>
        </header>
        <div class="ui-content">{props.children}</div>
      </section>
    </main>
  );
}
