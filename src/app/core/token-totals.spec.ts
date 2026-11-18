import { Injector } from "@angular/core";
import { describe, expect, it, vi } from "vitest";
import { TauriBridge } from "./tauri";
import {
  TokenTotalsService,
  windowTotals,
  compactTotal,
  parseTokensEvent,
  sumTotals,
  totalsTip,
} from "./token-totals";

const t = (input: number, output = 0, cache_read = 0, cache_creation = 0) => ({
  input,
  output,
  cache_read,
  cache_creation,
});

describe("token totals", () => {
  it("sums the panes of a space and ignores panes without a total", () => {
    expect(sumTotals([t(1, 2, 3, 4), undefined, t(10, 20, 30, 40)])).toEqual(t(11, 22, 33, 44));
    expect(sumTotals([undefined, undefined])).toBeNull();
    expect(sumTotals([])).toBeNull();
  });

  it("formats compactly and lists the four counts", () => {
    expect(compactTotal(t(1_000_000, 100_000, 100_000, 0))).toBe("1.2M");
    expect(compactTotal(t(842))).toBe("842");
    expect(totalsTip(t(1500, 20, 300_000, 4000))).toBe(
      "Tokens 306k: input 1.5k, output 20, cache read 300k, cache write 4k",
    );
  });

  it("reads a tokens event and rejects malformed ones", () => {
    expect(parseTokensEvent({ terminal_id: "t1", totals: t(1, 2, 3, 4) })).toEqual({
      id: "t1",
      totals: t(1, 2, 3, 4),
      samples: [],
    });
    expect(parseTokensEvent({ terminal_id: "t1", totals: { input: 1 } })).toBeNull();
    expect(parseTokensEvent({ terminal_id: "t1", totals: t(-1) })).toBeNull();
    expect(parseTokensEvent(null)).toBeNull();
  });

  function setup(watch: (id: string) => boolean | Error) {
    let listener: ((raw: unknown) => void) | undefined;
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command !== "tokens_watch") return undefined;
      const answer = watch(String(args?.["terminalId"]));
      if (answer instanceof Error) throw answer;
      return answer;
    });
    const bridge = {
      invoke,
      listen: vi.fn(async (_name: string, cb: (raw: unknown) => void) => {
        listener = cb;
        return () => undefined;
      }),
    };
    const injector = Injector.create({
      providers: [
        { provide: TauriBridge, useValue: bridge },
        { provide: TokenTotalsService, useClass: TokenTotalsService },
      ],
    });
    return {
      service: injector.get(TokenTotalsService),
      invoke,
      emit: (r: unknown) => listener?.(r),
    };
  }

  it("follows panes, keeps their totals and drops them when they leave", async () => {
    const { service, invoke, emit } = setup(() => true);
    await service.follow(new Map([["t1", "s1"]]));
    emit({ terminal_id: "t1", totals: t(5) });
    expect(service.totals().get("t1")).toEqual(t(5));
    await service.follow(new Map());
    expect(service.totals().size).toBe(0);
    expect(invoke).toHaveBeenCalledWith("tokens_unwatch", { terminalId: "t1" });
  });

  it("shows no total for a pane whose watch failed (offline machine) and asks again later", async () => {
    let fail = true;
    const { service, invoke } = setup(() => (fail ? new Error("offline") : true));
    await service.follow(new Map([["m::t1", "s1"]]));
    expect(service.totals().size).toBe(0);
    fail = false;
    await service.follow(new Map([["m::t1", "s1"]]));
    expect(invoke.mock.calls.filter(([c]) => c === "tokens_watch")).toHaveLength(2);
  });

  it("keeps timestamped samples per pane, also when a later watch fails, and drops them with the pane", async () => {
    const { service, emit } = setup(() => true);
    await service.follow(new Map([["t1", "s1"]]));
    emit({
      terminal_id: "t1",
      totals: t(5),
      samples: [
        { at: "2026-11-08T10:00:00.000Z", ...t(5) },
        { at: "bad", ...t(1) },
      ],
    });
    expect(service.samples().get("t1")).toEqual([
      { at: Date.parse("2026-11-08T10:00:00.000Z"), ...t(5) },
    ]);
    await service.follow(new Map());
    expect(service.samples().size).toBe(0);
  });

  it("windowTotals includes both ends and is null for an empty window", () => {
    const s = (at: number, input: number) => ({ at, ...t(input) });
    const list = [s(1, 1), s(2, 10), s(3, 100), s(4, 1000)];
    expect(windowTotals(list, 2, 3)?.input).toBe(110);
    expect(windowTotals(list, 5, 9)).toBeNull();
  });
});
