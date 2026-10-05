import { describe, expect, test } from "vitest";
import {
  albumSpans,
  centre,
  clampAnchor,
  firstRow,
  poolSize,
  reanchor,
  resize,
  ROW_PX,
  SPACE_ROWS,
  slotPosition,
  spaceRows,
} from "./virtual";

const LONG = 180_000;
const VIEW = 580;
const maxScrollTop = (length: number) => spaceRows(length) * ROW_PX - VIEW;

describe("space", () => {
  test("is the whole queue up to its cap", () => {
    expect(spaceRows(0)).toBe(0);
    expect(spaceRows(500)).toBe(500);
    expect(spaceRows(LONG)).toBe(SPACE_ROWS);
  });

  test("anchor stays within the queue", () => {
    expect(clampAnchor(-5, LONG)).toBe(0);
    expect(clampAnchor(LONG, LONG)).toBe(LONG - SPACE_ROWS);
    expect(clampAnchor(10, 500)).toBe(0);
    expect(clampAnchor(10, 0)).toBe(0);
  });
});

describe("rendered rows", () => {
  test("cover the viewport and margins, never more rows than exist", () => {
    expect(poolSize(VIEW, LONG)).toBe(42);
    expect(poolSize(VIEW, 20)).toBe(20);
    expect(poolSize(VIEW, 0)).toBe(0);
  });

  test("start a margin above the viewport, clamped to the space", () => {
    const pool = poolSize(VIEW, LONG);
    expect(firstRow(0, pool, LONG)).toBe(0);
    expect(firstRow(100 * ROW_PX + 30, pool, LONG)).toBe(85);
    const last = firstRow(maxScrollTop(LONG), pool, LONG);
    expect(last + pool).toBe(SPACE_ROWS);
    expect(firstRow(0, 20, 20)).toBe(0);
    expect(firstRow(0, 0, 0)).toBe(0);
  });

  test("slots hold each position of the range once, and keep it on a shift", () => {
    const at = (first: number) =>
      Array.from({ length: 42 }, (_, slot) => slotPosition(slot, first, 42));
    const range = at(1000);
    expect([...range].sort((a, b) => a - b)).toEqual(
      Array.from({ length: 42 }, (_, i) => 1000 + i),
    );
    const shifted = at(1001);
    expect(shifted.filter((p, slot) => p !== range[slot])).toEqual([1042]);
  });
});

describe("album spans", () => {
  // One character per position from `first`: S starts a group, - continues
  // one, ? is not served.
  const spans = (first: number, rows: string) =>
    albumSpans(first, first + rows.length, (p) =>
      rows[p - first] === "?" ? undefined : rows[p - first] === "S",
    ).map(({ start, end }) => [start, end]);

  test("run from each group start to the next, the last to the range's end", () => {
    expect(spans(100, "S--S-SS--")).toEqual([
      [100, 103],
      [103, 105],
      [105, 106],
      [106, 109],
    ]);
  });

  test("a group started above the range spans from its first row", () => {
    expect(spans(100, "---S-")).toEqual([
      [100, 103],
      [103, 105],
    ]);
  });

  test("rows not served end a span and belong to none", () => {
    expect(spans(100, "S-??--S??")).toEqual([
      [100, 102],
      [104, 106],
      [106, 107],
    ]);
    expect(spans(100, "???")).toEqual([]);
    expect(spans(100, "")).toEqual([]);
  });
});

describe("reanchor", () => {
  const onScreen = (anchor: number, scrollTop: number) =>
    anchor * ROW_PX + scrollTop;

  test("leaves the middle half alone", () => {
    expect(reanchor(50_000, (SPACE_ROWS / 4) * ROW_PX, LONG)).toBeNull();
    expect(reanchor(50_000, ((SPACE_ROWS * 3) / 4) * ROW_PX, LONG)).toBeNull();
  });

  test("recentres in whole rows without moving the screen", () => {
    const scrollTop = 17_000 * ROW_PX + 13.5;
    const next = reanchor(50_000, scrollTop, LONG)!;
    expect(next.anchor).toBe(50_000 + 17_000 - SPACE_ROWS / 2);
    expect(next.scrollTop).toBe((SPACE_ROWS / 2) * ROW_PX + 13.5);
    expect(onScreen(next.anchor, next.scrollTop)).toBe(
      onScreen(50_000, scrollTop),
    );
  });

  test("stops at the queue's ends, so they stay the space's edges", () => {
    expect(reanchor(0, 100, LONG)).toBeNull();
    const end = LONG - SPACE_ROWS;
    expect(reanchor(end, maxScrollTop(LONG), LONG)).toBeNull();
    const near = reanchor(end - 10, maxScrollTop(LONG), LONG)!;
    expect(near.anchor).toBe(end);
    expect(near.scrollTop).toBe(maxScrollTop(LONG) - 10 * ROW_PX);
  });

  test("never moves a queue that fits in the space", () => {
    expect(reanchor(0, 0, 500)).toBeNull();
    expect(reanchor(0, maxScrollTop(500), 500)).toBeNull();
    expect(reanchor(0, 0, 0)).toBeNull();
  });
});

describe("centre", () => {
  const middle = (p: { anchor: number; scrollTop: number }) =>
    p.anchor * ROW_PX + p.scrollTop + VIEW / 2;

  test("puts the row's centre mid-viewport, mid-space so no re-anchor follows", () => {
    const next = centre(90_000, VIEW, LONG);
    expect(middle(next)).toBe(90_000 * ROW_PX + ROW_PX / 2);
    expect(next.scrollTop).toBeGreaterThanOrEqual(0);
    expect(next.scrollTop).toBeLessThanOrEqual(maxScrollTop(LONG));
    expect(reanchor(next.anchor, next.scrollTop, LONG)).toBeNull();
  });

  test("leaves the first or last row flush at the queue's ends", () => {
    expect(centre(0, VIEW, LONG)).toEqual({ anchor: 0, scrollTop: 0 });
    expect(centre(3, VIEW, LONG)).toEqual({ anchor: 0, scrollTop: 0 });
    const end = { anchor: LONG - SPACE_ROWS, scrollTop: maxScrollTop(LONG) };
    expect(centre(LONG - 1, VIEW, LONG)).toEqual(end);
    expect(centre(LONG - 4, VIEW, LONG)).toEqual(end);
  });

  test("stays at the top when the queue is shorter than the list", () => {
    expect(centre(5, VIEW, 8)).toEqual({ anchor: 0, scrollTop: 0 });
  });

  test("scrolls a queue within one space without an anchor", () => {
    const next = centre(3000, VIEW, 5000);
    expect(next.anchor).toBe(0);
    expect(middle(next)).toBe(3000 * ROW_PX + ROW_PX / 2);
  });

  test("with room after the last row, the last rows centre or rest above it", () => {
    const room = 64;
    const last = centre(LONG - 1, VIEW, LONG, room);
    expect(last.anchor).toBe(LONG - SPACE_ROWS);
    expect(last.scrollTop).toBe(maxScrollTop(LONG) + room);
    expect(middle(centre(LONG - 6, VIEW, LONG, room))).toBe(
      (LONG - 6) * ROW_PX + ROW_PX / 2,
    );
    expect(centre(5, VIEW, 8, room)).toEqual({ anchor: 0, scrollTop: 0 });
  });
});

describe("resize", () => {
  test("keeps anchor and scroll when the viewport's top still fits", () => {
    expect(resize(50_000, 1234, VIEW, LONG + 7)).toEqual({
      anchor: 50_000,
      scrollTop: 1234,
    });
  });

  test("clamps the anchor and keeps the top row when the queue shrinks", () => {
    const next = resize(LONG - SPACE_ROWS, 9000 * ROW_PX, VIEW, LONG - 100);
    expect(next.anchor).toBe(LONG - 100 - SPACE_ROWS);
    expect(next.anchor * ROW_PX + next.scrollTop).toBe(
      (LONG - SPACE_ROWS + 9000) * ROW_PX,
    );
  });

  test("moves to the new end when the top row is gone", () => {
    const next = resize(LONG - SPACE_ROWS, maxScrollTop(LONG), VIEW, 30_000);
    expect(next.anchor).toBe(30_000 - SPACE_ROWS);
    expect(next.scrollTop).toBe(maxScrollTop(30_000));
  });

  test("shrinking below the space maps it onto the whole queue", () => {
    const next = resize(80_000, 9000 * ROW_PX, VIEW, 2000);
    expect(next).toEqual({ anchor: 0, scrollTop: maxScrollTop(2000) });
    expect(resize(0, 300, VIEW, 2000)).toEqual({ anchor: 0, scrollTop: 300 });
  });

  test("growing past the space keeps the top row", () => {
    expect(resize(0, 15_000 * ROW_PX, VIEW, LONG)).toEqual({
      anchor: 0,
      scrollTop: 15_000 * ROW_PX,
    });
  });

  test("keeps an offset inside the room after the last row", () => {
    const room = 64;
    const end = maxScrollTop(LONG) + room;
    expect(resize(LONG - SPACE_ROWS, end, VIEW, LONG, room).scrollTop).toBe(
      end,
    );
    expect(resize(LONG - SPACE_ROWS, end, VIEW, LONG).scrollTop).toBe(
      maxScrollTop(LONG),
    );
  });

  test("an empty queue puts everything at zero", () => {
    expect(resize(50_000, 4000, VIEW, 0)).toEqual({ anchor: 0, scrollTop: 0 });
  });
});
