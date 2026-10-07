import { Show } from "solid-js";
import { IconButton } from "~/components/IconButton";

export interface ResultPaginationProps {
  canPrevious: boolean;
  canNext: boolean;
  busy: boolean;
  previousLabel: string;
  nextLabel: string;
  ariaLabel: string;
  class?: string;
  onPrevious: () => void;
  onNext: () => void;
}

/**
 * Shared Previous/Next vocabulary for already-fetched result pages.
 * Knows nothing about rows, cursors, continuations, queries, errors,
 * selection, retry, or source kinds: the owning controller or query handle
 * decides availability and performs the page change. A single page renders
 * no navigation at all.
 */
export function ResultPagination(props: ResultPaginationProps) {
  const navClass = () =>
    ["result-pagination", props.class].filter(Boolean).join(" ");

  return (
    <Show when={props.canPrevious || props.canNext}>
      <nav class={navClass()} aria-label={props.ariaLabel}>
        <IconButton
          class="result-pagination-button"
          icon="chevron-left"
          label={props.previousLabel}
          title={props.previousLabel}
          disabled={!props.canPrevious || props.busy}
          onClick={() => props.onPrevious()}
        />
        <IconButton
          class="result-pagination-button"
          icon="chevron-right"
          label={props.nextLabel}
          title={props.nextLabel}
          disabled={!props.canNext || props.busy}
          onClick={() => props.onNext()}
        />
      </nav>
    </Show>
  );
}
