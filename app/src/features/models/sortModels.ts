import type { ModelEntry_Serialize as ModelEntry } from "../../ipc/bindings";

export type SortKey = "cheapest" | "fastest" | "newest" | "context" | "favorites";

export const SORTS: { id: SortKey; label: string }[] = [
  { id: "cheapest", label: "Cheapest first" },
  { id: "fastest", label: "Fastest" },
  { id: "newest", label: "Newest" },
  { id: "context", label: "Context size" },
  { id: "favorites", label: "Favorites" },
];

export const modelKey = (m: { providerId: string; id: string }) => `${m.providerId}/${m.id}`;

export type PickerFilter = {
  query: string;
  freeOnly: boolean;
  /** Only these providers; empty = all. */
  providers: string[];
};

export type PickerContext = {
  favorites: Set<string>;
  /** Measured output tokens/second per model key, from past turns on this machine. */
  speed: Record<string, number>;
};

/** Blended price for sorting; unknown pricing sorts after every known price. */
function price(m: ModelEntry): number {
  if (m.free) return 0;
  if (!m.pricingKnown || m.priceIn == null || m.priceOut == null) return Number.POSITIVE_INFINITY;
  return m.priceIn + m.priceOut;
}

export function filterModels(list: ModelEntry[], f: PickerFilter): ModelEntry[] {
  const words = f.query.toLowerCase().split(/\s+/).filter(Boolean);
  return list.filter((m) => {
    if (f.freeOnly && !m.free) return false;
    if (f.providers.length > 0 && !f.providers.includes(m.providerId)) return false;
    const hay = `${m.id} ${m.name} ${m.providerId}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

export function sortModels(list: ModelEntry[], key: SortKey, ctx: PickerContext): ModelEntry[] {
  const byName = (a: ModelEntry, b: ModelEntry) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id);
  const fav = (m: ModelEntry) => (ctx.favorites.has(modelKey(m)) ? 0 : 1);
  const cmp: Record<SortKey, (a: ModelEntry, b: ModelEntry) => number> = {
    cheapest: (a, b) => price(a) - price(b) || byName(a, b),
    // Unmeasured models go last.
    fastest: (a, b) => (ctx.speed[modelKey(b)] ?? -1) - (ctx.speed[modelKey(a)] ?? -1) || byName(a, b),
    newest: (a, b) => (b.released ?? "").localeCompare(a.released ?? "") || byName(a, b),
    context: (a, b) => (b.context ?? 0) - (a.context ?? 0) || byName(a, b),
    favorites: (a, b) => fav(a) - fav(b) || price(a) - price(b) || byName(a, b),
  };
  // Infinity - Infinity is NaN; treat as equal.
  return [...list].sort((a, b) => cmp[key](a, b) || 0);
}

const SPEED_KEY = "lampcode.modelSpeed";

/** Per-machine convenience only: never holds anything sensitive. */
export function loadSpeeds(): Record<string, number> {
  try {
    const raw = localStorage.getItem(SPEED_KEY);
    return raw ? (JSON.parse(raw) as Record<string, number>) : {};
  } catch {
    return {};
  }
}

export function recordSpeed(key: string, tokensPerSecond: number) {
  if (!Number.isFinite(tokensPerSecond) || tokensPerSecond <= 0) return;
  try {
    const all = loadSpeeds();
    const prev = all[key];
    all[key] = prev ? prev * 0.6 + tokensPerSecond * 0.4 : tokensPerSecond;
    localStorage.setItem(SPEED_KEY, JSON.stringify(all));
  } catch {
    // Storage blocked: speed sorting just has less data.
  }
}
