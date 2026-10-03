import { describe, expect, test } from "vitest";
import { formatTime, position } from "./progress";
import type { Snapshot } from "./ws";

const snapshot = (overrides: Partial<Snapshot>): Snapshot => ({
  state: "play",
  title: "t",
  artist: "a",
  album: null,
  folder: null,
  elapsed: 10,
  duration: 100,
  volume: null,
  random: false,
  repeat: false,
  single: false,
  at: 1000,
  ...overrides,
});

describe("position", () => {
  test("advances with local time while playing", () => {
    expect(position(snapshot({}), 3500)).toBe(12.5);
  });

  test("holds still while paused or stopped", () => {
    expect(position(snapshot({ state: "pause" }), 60_000)).toBe(10);
    expect(position(snapshot({ state: "stop" }), 60_000)).toBe(10);
  });

  test("stops at the song's end", () => {
    expect(position(snapshot({}), 1_000_000)).toBe(100);
  });

  test("keeps advancing when the duration is unknown", () => {
    expect(position(snapshot({ duration: null }), 1_001_000)).toBe(1010);
  });

  test("never goes back before the anchor", () => {
    expect(position(snapshot({}), 0)).toBe(10);
  });

  test("is unknown without a song", () => {
    expect(position(snapshot({ elapsed: null }), 2000)).toBeNull();
  });
});

test("formatTime pads seconds and adds hours past one hour", () => {
  expect(formatTime(0)).toBe("0:00");
  expect(formatTime(59.99)).toBe("0:59");
  expect(formatTime(605)).toBe("10:05");
  expect(formatTime(3725)).toBe("1:02:05");
});
