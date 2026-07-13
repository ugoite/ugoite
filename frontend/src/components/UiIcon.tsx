import type { JSX } from "solid-js";

export type UiIconName =
  | "home"
  | "forms"
  | "search"
  | "settings"
  | "spaces"
  | "about"
  | "menu"
  | "plus"
  | "entry"
  | "asset"
  | "sql"
  | "members"
  | "agent"
  | "credential"
  | "storage"
  | "appearance"
  | "history"
  | "close";

const paths: Record<UiIconName, JSX.Element> = {
  home: (
    <>
      <path d="M3 10.5 12 3l9 7.5" />
      <path d="M5 10v10h14V10" />
      <path d="M9 20v-6h6v6" />
    </>
  ),
  forms: (
    <>
      <path d="M4 5h16" />
      <path d="M4 12h16" />
      <path d="M4 19h16" />
      <path d="M7 5v14" />
    </>
  ),
  search: (
    <>
      <circle cx="10.5" cy="10.5" r="6.5" />
      <path d="m16 16 5 5" />
    </>
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path
        d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1-2 3.4-.2-.1a1.8 1.8 0 0 0-1.9-.1 8 8 0 0 1-1.4.6 1.7 1.7 0 0 0-1.2 1.5v.2H9v-.2a1.7 1.7 0 0 0-1.2-1.5 8 8 0 0 1-1.4-.6 1.8 1.8 0 0 0-1.9.1l-.2.1-2-3.4.1-.1a1.7 1.7 0 0 0 .3-1.8A8 8 0 0 1 2.4 13 1.7 1.7 0 0 0 1 11.5H.8V7.6H1a1.7 1.7 0 0 0 1.4-1.5A8 8 0 0 1 2.7 5a1.7 1.7 0 0 0-.3-1.8l-.1-.1 2-3.4.2.1a1.8 1.8 0 0 0 1.9.1 8 8 0 0 1 1.4-.6A1.7 1.7 0 0 0 9 .2V0h3.9v.2a1.7 1.7 0 0 0 1.2 1.5 8 8 0 0 1 1.4.6 1.8 1.8 0 0 0 1.9-.1l.2-.1 2 3.4-.1.1a1.7 1.7 0 0 0-.3 1.8c.2.5.4 1 .4 1.6a1.7 1.7 0 0 0 1.4 1.5h.2v3.9H21a1.7 1.7 0 0 0-1.6 1.6Z"
        transform="translate(1.5 1.5) scale(.88)"
      />
    </>
  ),
  spaces: (
    <>
      <path d="M4 5h16" />
      <path d="M4 12h16" />
      <path d="M4 19h16" />
      <path d="M7 5v14" />
    </>
  ),
  about: (
    <>
      <path d="M6 4h12v16H6z" />
      <path d="M9 8h6M9 12h6M9 16h4" />
    </>
  ),
  menu: (
    <>
      <path d="M4 5h16" />
      <path d="M4 12h16" />
      <path d="M4 19h16" />
    </>
  ),
  plus: (
    <>
      <path d="M12 5v14" />
      <path d="M5 12h14" />
    </>
  ),
  entry: (
    <>
      <path d="m12 3 7 7-7 11-7-11Z" />
    </>
  ),
  asset: (
    <>
      <rect x="4" y="4" width="16" height="16" rx="3" />
      <path d="m7 16 4-4 3 3 3-3" />
    </>
  ),
  sql: (
    <>
      <path d="M8 4 5 20M16 4l-3 16M3 9h17M2 15h17" />
    </>
  ),
  members: (
    <>
      <circle cx="12" cy="8" r="4" />
      <path d="M4 21c1.8-4 14.2-4 16 0" />
    </>
  ),
  agent: (
    <>
      <path d="M12 3v4" />
      <rect x="5" y="7" width="14" height="11" rx="4" />
      <path d="M9 12h.01M15 12h.01M9 16h6" />
    </>
  ),
  credential: (
    <>
      <circle cx="8" cy="12" r="4" />
      <path d="M12 12h8M17 12v3M20 12v3" />
    </>
  ),
  storage: (
    <>
      <ellipse cx="12" cy="6" rx="7" ry="3" />
      <path d="M5 6v6c0 1.7 3.1 3 7 3s7-1.3 7-3V6" />
      <path d="M5 12v6c0 1.7 3.1 3 7 3s7-1.3 7-3v-6" />
    </>
  ),
  appearance: (
    <>
      <path d="M4 5h16" />
      <path d="M4 12h16" />
      <path d="M4 19h16" />
      <path d="M7 5v14" />
    </>
  ),
  history: (
    <>
      <path d="M4 12a8 8 0 1 0 2.3-5.7L4 8.5" />
      <path d="M4 4v4.5h4.5M12 8v5l3 2" />
    </>
  ),
  close: (
    <>
      <path d="m6 6 12 12M18 6 6 18" />
    </>
  ),
};

export function UiIcon(props: { name: UiIconName; class?: string }) {
  return (
    <svg class={props.class ?? "icon"} viewBox="0 0 24 24" aria-hidden="true">
      {paths[props.name]}
    </svg>
  );
}
