import { A } from "@solidjs/router";
import { Show } from "solid-js";
import type { JSX } from "solid-js";

interface RowListProps {
  /** Accessible name for the list (e.g. the section heading). */
  label: string;
  children: JSX.Element;
}

/**
 * Shared row-list primitive (POL-UI-003, POL-UI-006, POL-UI-007).
 * Border-separated rows without card chrome: each `RowListItem` carries one
 * full-row activation control plus optional unboxed secondary actions.
 * Rows use native links/buttons so keyboard Enter/Space activation and the
 * focus path come from the platform, not custom key handlers.
 */
export function RowList(props: RowListProps) {
  return (
    <div class="rowList" role="list" aria-label={props.label}>
      {props.children}
    </div>
  );
}

interface RowListItemProps {
  /**
   * The full-row activation control (`RowListLink` or `RowListButton`).
   * Secondary `actions` render as siblings, never nested inside `main`,
   * so a secondary click never triggers row activation.
   */
  main: JSX.Element;
  actions?: JSX.Element;
}

export function RowListItem(props: RowListItemProps) {
  return (
    <div class="rowListItem" role="listitem">
      {props.main}
      <Show when={props.actions}>
        {(actions) => <span class="rowListActions">{actions()}</span>}
      </Show>
    </div>
  );
}

interface RowListMainProps {
  /** Human-meaningful name; raw IDs stay out of rows (POL-UI-007). */
  primary: JSX.Element;
  /** Optional supporting line under the primary name. */
  secondary?: JSX.Element;
  /** Optional compact right-aligned meta (date, count). */
  meta?: JSX.Element;
  /** Optional unboxed chevron affordance; never a boxed control. */
  chevron?: boolean;
  ariaLabel?: string;
  title?: string;
}

function RowListMainContent(props: RowListMainProps) {
  return (
    <>
      <span class="rowListText">
        <span class="rowListPrimary">{props.primary}</span>
        <Show when={props.secondary}>
          {(secondary) => <span class="rowListSecondary">{secondary()}</span>}
        </Show>
      </span>
      <Show when={props.meta}>
        {(meta) => <span class="rowListMeta">{meta()}</span>}
      </Show>
      <Show when={props.chevron}>
        <span class="rowListChevron" aria-hidden="true">
          ›
        </span>
      </Show>
    </>
  );
}

interface RowListLinkProps extends RowListMainProps {
  href: string;
}

/** Full-row link activation: the whole row navigates, no Open column. */
export function RowListLink(props: RowListLinkProps) {
  return (
    <A
      href={props.href}
      class="rowListMain"
      aria-label={props.ariaLabel}
      title={props.title}
    >
      <RowListMainContent
        primary={props.primary}
        secondary={props.secondary}
        meta={props.meta}
        chevron={props.chevron}
      />
    </A>
  );
}

interface RowListButtonProps extends RowListMainProps {
  onActivate: () => void;
}

/** Full-row button activation for caller-driven navigation/selection. */
export function RowListButton(props: RowListButtonProps) {
  return (
    <button
      type="button"
      class="rowListMain"
      onClick={() => props.onActivate()}
      aria-label={props.ariaLabel}
      title={props.title}
    >
      <RowListMainContent
        primary={props.primary}
        secondary={props.secondary}
        meta={props.meta}
        chevron={props.chevron}
      />
    </button>
  );
}
