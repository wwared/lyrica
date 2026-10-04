// Arithmetic of the queue's virtual list (architecture.md → "Live queue").
// "Space" is the scrolled content: `spaceRows(length)` rows mapped onto the
// queue through the anchor, the queue position of the space's first row.

/** Matches `.queue-row` in style.css. */
export const ROW_PX = 56;
/** Browsers cap element heights (~300k rows in Firefox); this stays far below. */
export const SPACE_ROWS = 18_000;
/** Rows rendered beyond the viewport on each side. */
export const MARGIN_ROWS = 15;

export const spaceRows = (length: number) => Math.min(length, SPACE_ROWS);

export const clampAnchor = (anchor: number, length: number) =>
  Math.max(0, Math.min(anchor, length - spaceRows(length)));

/** Rows kept in the DOM: the viewport, a partial row, the margins. */
export const poolSize = (viewportPx: number, length: number) =>
  Math.min(
    Math.ceil(viewportPx / ROW_PX) + 2 * MARGIN_ROWS + 1,
    spaceRows(length),
  );

/** The first rendered row of the space; the pool never runs past its end. */
export const firstRow = (scrollTop: number, pool: number, length: number) =>
  Math.max(
    0,
    Math.min(
      Math.floor(scrollTop / ROW_PX) - MARGIN_ROWS,
      spaceRows(length) - pool,
    ),
  );

/**
 * The queue position held by `slot`, for a pool of `pool` rows starting at
 * queue position `first`. Slots follow the queue position, so rows keep their
 * slot across scrolls and re-anchors.
 */
export const slotPosition = (slot: number, first: number, pool: number) =>
  first + ((((slot - first) % pool) + pool) % pool);

export interface Placement {
  anchor: number;
  scrollTop: number;
}

/**
 * After scrolling stops: moves the anchor so the viewport sits mid-space,
 * leaving the screen unchanged; `null` while the viewport's top is in the
 * space's middle half or the anchor can't move.
 */
export function reanchor(
  anchor: number,
  scrollTop: number,
  length: number,
): Placement | null {
  const rows = spaceRows(length);
  const top = scrollTop / ROW_PX;
  if (top >= rows / 4 && top <= (rows * 3) / 4) return null;
  const next = clampAnchor(
    anchor + Math.floor(top) - Math.floor(rows / 2),
    length,
  );
  if (next === anchor) return null;
  return { anchor: next, scrollTop: scrollTop - (next - anchor) * ROW_PX };
}

/**
 * After the queue length changes: keeps the queue offset at the viewport's
 * top where it still exists. The clamped anchor always leaves it in the space.
 */
export function resize(
  anchor: number,
  scrollTop: number,
  viewportPx: number,
  length: number,
): Placement {
  const offset = Math.min(
    anchor * ROW_PX + scrollTop,
    Math.max(0, length * ROW_PX - viewportPx),
  );
  const next = clampAnchor(anchor, length);
  return { anchor: next, scrollTop: offset - next * ROW_PX };
}
