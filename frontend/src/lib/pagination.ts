/**
 * Interpret an additive page request made with one extra item.
 *
 * Durable collection endpoints continue to return their existing arrays. The
 * extra item is only a runtime signal for the UI and is never persisted or
 * treated as a Knowledge authority.
 */
export type Page<T> = {
  items: T[];
  hasMore: boolean;
};

export const pageFromArray = <T>(items: T[], pageSize: number): Page<T> => ({
  items: items.slice(0, pageSize),
  hasMore: items.length > pageSize,
});
