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
  | "refresh"
  | "info"
  | "trash"
  | "preview"
  | "download"
  | "close";

const paths: Record<UiIconName, () => JSX.Element> = {
  home: () => (
    <>
      <path d="M3 10.5 12 3l9 7.5" />
      <path d="M5 10v10h14V10" />
      <path d="M9 20v-6h6v6" />
    </>
  ),
  forms: () => (
    <>
      <rect x="4" y="4" width="16" height="16" rx="2" />
      <path d="M8 9h8M8 12h8M8 15h5" />
    </>
  ),
  search: () => (
    <>
      <circle cx="10.5" cy="10.5" r="6.5" />
      <path d="m16 16 5 5" />
    </>
  ),
  settings: () => (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M12 3.5l1.2 2.4 2.7.4 1.8-1.7 1.8 1.8-1.7 1.8.4 2.7 2.3 1.1v2l-2.3 1.1-.4 2.7 1.7 1.8-1.8 1.8-1.8-1.7-2.7.4L12 20.5l-1.2-2.4-2.7-.4-1.8 1.7-1.8-1.8 1.7-1.8-.4-2.7L3.5 12V10l2.3-1.1.4-2.7-1.7-1.8 1.8-1.8 1.8 1.7 2.7-.4z" />
    </>
  ),
  spaces: () => (
    <>
      <path d="M4 7.5 12 4l8 3.5-8 3.5-8-3.5z" />
      <path d="M4 12.5 12 16l8-3.5" />
      <path d="M4 17 12 20l8-3" />
    </>
  ),
  about: () => (
    <>
      <path d="M6 4h12v16H6z" />
      <path d="M9 8h6M9 12h6M9 16h4" />
    </>
  ),
  menu: () => (
    <>
      <path d="M4 5h16" />
      <path d="M4 12h16" />
      <path d="M4 19h16" />
    </>
  ),
  plus: () => (
    <>
      <path d="M12 5v14" />
      <path d="M5 12h14" />
    </>
  ),
  entry: () => (
    <>
      <path d="m12 3 7 7-7 11-7-11Z" />
    </>
  ),
  asset: () => (
    <>
      <rect x="4" y="4" width="16" height="16" rx="3" />
      <path d="m7 16 4-4 3 3 3-3" />
    </>
  ),
  sql: () => (
    <>
      <path d="M8 4 5 20M16 4l-3 16M3 9h17M2 15h17" />
    </>
  ),
  members: () => (
    <>
      <circle cx="12" cy="8" r="4" />
      <path d="M4 21c1.8-4 14.2-4 16 0" />
    </>
  ),
  agent: () => (
    <>
      <path d="M12 3v4" />
      <rect x="5" y="7" width="14" height="11" rx="4" />
      <path d="M9 12h.01M15 12h.01M9 16h6" />
    </>
  ),
  credential: () => (
    <>
      <circle cx="8" cy="12" r="4" />
      <path d="M12 12h8M17 12v3M20 12v3" />
    </>
  ),
  storage: () => (
    <>
      <ellipse cx="12" cy="6" rx="7" ry="3" />
      <path d="M5 6v6c0 1.7 3.1 3 7 3s7-1.3 7-3V6" />
      <path d="M5 12v6c0 1.7 3.1 3 7 3s7-1.3 7-3v-6" />
    </>
  ),
  appearance: () => (
    <>
      <path d="M4 8h10M18 8h2M4 16h4M12 16h8" />
      <circle cx="16" cy="8" r="2" />
      <circle cx="10" cy="16" r="2" />
    </>
  ),
  history: () => (
    <>
      <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
      <path d="M3 3v5h5" />
      <path d="M12 7v5l3 2" />
    </>
  ),
  refresh: () => (
    <>
      <path d="M20 6v5h-5" />
      <path d="M4 18v-5h5" />
      <path d="M18.5 9A7 7 0 0 0 6.2 6.2L4 11" />
      <path d="M5.5 15A7 7 0 0 0 17.8 17.8L20 13" />
    </>
  ),
  info: () => (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v6M12 7h.01" />
    </>
  ),
  trash: () => (
    <>
      <path d="M4 7h16M9 7V4h6v3" />
      <path d="M8 10v8M12 10v8M16 10v8M6 7l1 14h10l1-14" />
    </>
  ),
  preview: () => (
    <>
      <path d="M2.5 12s3.3-5 9.5-5 9.5 5 9.5 5-3.3 5-9.5 5-9.5-5-9.5-5Z" />
      <circle cx="12" cy="12" r="2.5" />
    </>
  ),
  download: () => (
    <>
      <path d="M12 3v12" />
      <path d="m7 10 5 5 5-5" />
      <path d="M5 21h14" />
    </>
  ),
  close: () => (
    <>
      <path d="m6 6 12 12M18 6 6 18" />
    </>
  ),
};

export function UiIcon(props: { name: UiIconName; class?: string }) {
  return (
    <svg
      class={props.class ?? "icon"}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.9"
      stroke-linecap="round"
      stroke-linejoin="round"
      vector-effect="non-scaling-stroke"
      aria-hidden="true"
    >
      {paths[props.name]()}
    </svg>
  );
}
