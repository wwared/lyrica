import { describe, expect, test } from "vitest";
import { CONNECTION_LOST, requests } from "./ws";

describe("requests", () => {
  test("a load settles with its own name's reply", async () => {
    const pending = requests();
    const a = pending.load("a");
    const b = pending.load("b");
    pending.loaded("b", "Playlist is too large");
    pending.loaded("a", null);
    expect(await a).toBe(null);
    expect(await b).toBe("Playlist is too large");
  });

  test("a listing answers every pending one", async () => {
    const pending = requests();
    const first = pending.listing();
    const second = pending.listing();
    pending.listed(["x"]);
    expect(await first).toEqual(["x"]);
    expect(await second).toEqual(["x"]);
  });

  test("a lost connection settles everything pending, once", async () => {
    const pending = requests();
    const names = pending.listing();
    const error = pending.load("a");
    pending.lost();
    pending.loaded("a", null);
    expect(await names).toBe(null);
    expect(await error).toBe(CONNECTION_LOST);
    const after = pending.load("a");
    pending.loaded("a", null);
    expect(await after).toBe(null);
  });
});
