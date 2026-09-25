export const VIRTUAL_ROW_HEIGHT = 48;
export const VIRTUAL_ROW_OVERSCAN = 6;
export const VIRTUAL_ROW_THRESHOLD = 100;

export interface VisibleRowRange {
  start: number;
  end: number;
  topSpacerHeight: number;
  bottomSpacerHeight: number;
}

export const calculateVisibleRowRange = (
  rowCount: number,
  scrollTop: number,
  viewportHeight: number,
  rowHeight = VIRTUAL_ROW_HEIGHT,
  overscan = VIRTUAL_ROW_OVERSCAN,
): VisibleRowRange => {
  if (rowCount <= 0) {
    return { start: 0, end: 0, topSpacerHeight: 0, bottomSpacerHeight: 0 };
  }
  const visibleCount = Math.max(1, Math.ceil(viewportHeight / rowHeight));
  const boundedScrollTop = Math.min(
    Math.max(0, scrollTop),
    Math.max(0, rowCount * rowHeight - viewportHeight),
  );
  const start = Math.max(
    0,
    Math.floor(boundedScrollTop / rowHeight) - overscan,
  );
  const end = Math.min(rowCount, start + visibleCount + overscan * 2);
  return {
    start,
    end,
    topSpacerHeight: start * rowHeight,
    bottomSpacerHeight: (rowCount - end) * rowHeight,
  };
};
