import { A } from "@solidjs/router";
import { type JSX, Show } from "solid-js";
import { UiIcon } from "~/components/UiIcon";
import { AccountMenu } from "~/components/AccountMenu";
import { t } from "~/lib/i18n";

export function GlobalShell(
  props: {
    children: JSX.Element;
    active?: "spaces";
    authenticated?: boolean;
  },
) {
  return (
    <main class="app workspaceApp">
      <div class="desktopSidebar">
        <aside class="sidebar">
          <a class="brand" href="/spaces">
            <img
              class="brandMark"
              src="/brand/ugoite-mark.svg"
              alt=""
              aria-hidden="true"
            />
            <span>Ugoite</span>
          </a>
          <nav class="sideFoot globalNav" aria-label={t("nav.spaces")}>
            <A
              class="navItem"
              classList={{ active: props.active === "spaces" }}
              href="/spaces"
              end
            >
              <UiIcon name="spaces" />
              <span>{t("nav.spaces")}</span>
            </A>
          </nav>
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
          <div class="topbarTools">
            <span class="pill iconpill">
              <span class="assistantDot" aria-hidden="true" />
              <span class="ui-sr-only">{t("konase.title")}</span>
            </span>
            <Show
              when={props.authenticated !== false}
              fallback={
                <a class="btn" href="/login">{t("globalShell.signIn")}</a>
              }
            >
              <AccountMenu />
            </Show>
          </div>
        </header>
        <div class="content">{props.children}</div>
      </section>
      <nav
        class="bottomNav globalBottomNav"
        aria-label={t("nav.spaces")}
      >
        <A
          classList={{ active: props.active === "spaces" }}
          href="/spaces"
          aria-current={props.active === "spaces" ? "page" : undefined}
          end
        >
          <UiIcon name="spaces" />
          <span>{t("nav.spaces")}</span>
        </A>
      </nav>
    </main>
  );
}
