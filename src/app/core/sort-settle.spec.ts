import { describe, expect, it } from "vitest";
import { SORT_SETTLE_MS, SortSettle } from "./sort-settle";

const keys = (o: Record<string, number>) => new Map(Object.entries(o));

describe("SortSettle", () => {
  it("applies the first keys at once", () => {
    const s = new SortSettle<number>(() => 0);
    expect(s.update(keys({ a: 1 }))).toEqual({ keys: keys({ a: 1 }), wait: 0 });
  });

  it("holds a status change until none changed for the settle time", () => {
    let now = 0;
    const s = new SortSettle<number>(() => now);
    s.update(keys({ a: 1, b: 2 }));
    now = 1000;
    expect(s.update(keys({ a: 2, b: 2 }))).toEqual({
      keys: keys({ a: 1, b: 2 }),
      wait: SORT_SETTLE_MS,
    });
    now = 3000;
    expect(s.update(keys({ a: 2, b: 2 })).wait).toBe(1000);
    now = 4000;
    expect(s.update(keys({ a: 2, b: 2 }))).toEqual({ keys: keys({ a: 2, b: 2 }), wait: 0 });
  });

  it("restarts the wait when another status changes", () => {
    let now = 0;
    const s = new SortSettle<number>(() => now);
    s.update(keys({ a: 1, b: 2 }));
    now = 1000;
    s.update(keys({ a: 2, b: 2 }));
    now = 3500;
    expect(s.update(keys({ a: 2, b: 1 }))).toEqual({
      keys: keys({ a: 1, b: 2 }),
      wait: SORT_SETTLE_MS,
    });
    now = 6500;
    expect(s.update(keys({ a: 2, b: 1 })).keys).toEqual(keys({ a: 2, b: 1 }));
  });

  it("forgets a change that was undone", () => {
    let now = 0;
    const s = new SortSettle<number>(() => now);
    s.update(keys({ a: 1 }));
    now = 100;
    s.update(keys({ a: 2 }));
    now = 200;
    expect(s.update(keys({ a: 1 }))).toEqual({ keys: keys({ a: 1 }), wait: 0 });
  });

  it("applies a new or closed item and an immediate update at once", () => {
    let now = 0;
    const s = new SortSettle<number>(() => now);
    s.update(keys({ a: 1, b: 2 }));
    now = 10;
    expect(s.update(keys({ a: 1, b: 2, c: 3 })).keys).toEqual(keys({ a: 1, b: 2, c: 3 }));
    expect(s.update(keys({ a: 1, c: 3 })).keys).toEqual(keys({ a: 1, c: 3 }));
    expect(s.update(keys({ a: 5, c: 3 }), true)).toEqual({ keys: keys({ a: 5, c: 3 }), wait: 0 });
  });
});
