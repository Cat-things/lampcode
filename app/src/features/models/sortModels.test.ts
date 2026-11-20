import { describe, expect, it } from "vitest";
import type { ModelEntry_Serialize as ModelEntry } from "../../ipc/bindings";
import { filterModels, modelKey, sortModels } from "./sortModels";

function m(id: string, over: Partial<ModelEntry> = {}): ModelEntry {
  return {
    providerId: "p",
    id,
    name: id,
    priceIn: 1,
    priceOut: 1,
    pricingKnown: true,
    free: false,
    context: 8000,
    tools: true,
    vision: false,
    reasoning: false,
    released: "2025-01-01",
    live: true,
    ...over,
  };
}

const list = [
  m("pricey", { priceIn: 10, priceOut: 30, context: 200_000, released: "2026-05-01" }),
  m("unknown", { pricingKnown: false, priceIn: null, priceOut: null, context: null, released: null }),
  m("free", { priceIn: 0, priceOut: 0, free: true, context: 32_000 }),
  m("cheap", { priceIn: 0.1, priceOut: 0.2, providerId: "q", released: "2026-01-01" }),
];
const ctx = { favorites: new Set<string>(), speed: {} as Record<string, number> };
const ids = (xs: ModelEntry[]) => xs.map((x) => x.id);

describe("model picker sorting", () => {
  it("cheapest puts free first and unknown pricing last", () => {
    expect(ids(sortModels(list, "cheapest", ctx))).toEqual(["free", "cheap", "pricey", "unknown"]);
  });

  it("context size, largest first", () => {
    expect(ids(sortModels(list, "context", ctx))).toEqual(["pricey", "free", "cheap", "unknown"]);
  });

  it("newest by release date, undated last", () => {
    expect(ids(sortModels(list, "newest", ctx))[0]).toBe("pricey");
    expect(ids(sortModels(list, "newest", ctx)).at(-1)).toBe("unknown");
  });

  it("favorites first, then by price", () => {
    const favs = { ...ctx, favorites: new Set([modelKey(list[0]), modelKey(list[1])]) };
    expect(ids(sortModels(list, "favorites", favs))).toEqual(["pricey", "unknown", "free", "cheap"]);
  });

  it("fastest uses measured speed and puts unmeasured last", () => {
    const fast = { ...ctx, speed: { [modelKey(list[3])]: 300, [modelKey(list[0])]: 40 } };
    expect(ids(sortModels(list, "fastest", fast)).slice(0, 2)).toEqual(["cheap", "pricey"]);
  });

  it("does not mutate the input", () => {
    const copy = [...list];
    sortModels(list, "cheapest", ctx);
    expect(list).toEqual(copy);
  });
});

describe("model picker filtering", () => {
  it("free only", () => {
    expect(ids(filterModels(list, { query: "", freeOnly: true, providers: [] }))).toEqual(["free"]);
  });

  it("search matches every word, any order", () => {
    expect(ids(filterModels(list, { query: "q CHE", freeOnly: false, providers: [] }))).toEqual(["cheap"]);
    expect(filterModels(list, { query: "nothing-matches", freeOnly: false, providers: [] })).toEqual([]);
  });

  it("provider filter", () => {
    expect(ids(filterModels(list, { query: "", freeOnly: false, providers: ["q"] }))).toEqual(["cheap"]);
  });
});
