import type { JSX } from "solid-js";
import { UiIcon } from "~/components/UiIcon";

export function GlobalShell(
  props: { title: string; children: JSX.Element; active?: "spaces" | "about" },
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
              <span>Home</span>
            </a>
            <a class="navItem" href="/spaces">
              <UiIcon name="forms" />
              <span>Forms</span>
            </a>
            <a class="navItem" href="/spaces">
              <UiIcon name="search" />
              <span>Search</span>
            </a>
            <a class="navItem" href="/spaces">
              <UiIcon name="settings" />
              <span>Settings</span>
            </a>
          </nav>
          <div class="sideFoot">
            <a
              class="navItem"
              classList={{ active: props.active === "spaces" }}
              href="/spaces"
            >
              <UiIcon name="spaces" />
              <span>Spaces</span>
            </a>
            <a
              class="navItem"
              classList={{ active: props.active === "about" }}
              href="/about"
            >
              <UiIcon name="about" />
              <span>About</span>
            </a>
          </div>
        </aside>
      </div>
      <section class="main">
        <header class="topbar">
          <a class="btn iconBtn mobileMenu" href="/spaces" aria-label="Menu">
            <UiIcon name="menu" />
          </a>
          <select class="spaceSelect" aria-label="Space">
            <option>Ugoite</option>
          </select>
          <div class="crumbTop">{props.title}</div>
          <a class="avatar" href="/settings/security" aria-label="Account">S</a>
        </header>
        <div class="content">{props.children}</div>
      </section>
      <nav class="bottomNav">
        <a class="active" href="/spaces">
          <UiIcon name="home" />
          <span>Home</span>
        </a>
        <a href="/spaces">
          <UiIcon name="forms" />
          <span>Forms</span>
        </a>
        <a href="/spaces">
          <UiIcon name="search" />
          <span>Search</span>
        </a>
        <a href="/spaces">
          <UiIcon name="settings" />
          <span>Settings</span>
        </a>
      </nav>
    </main>
  );
}
