import { useEffect, useMemo, useRef, useState } from "react";
import {
  commands,
  type ModelEntry_Serialize as ModelEntry,
  type ModelRef,
  type RateLimitInfo_Serialize as RateLimitInfo,
} from "../../ipc/bindings";
import { formatContext, formatPrice } from "../../lib/format";
import { IconEye, IconStar, IconTool } from "../../lib/icons";
import { useSettings } from "../../settings/SettingsProvider";
import { useAppData } from "../../state/AppData";
import { ProviderRail, usable } from "../providers/Rail";
import { SORTS, filterModels, loadSpeeds, modelKey, sortModels, type SortKey } from "./sortModels";

/** Providers whose live list was already requested this app session. */
const fetchedLive = new Set<string>();

export function headroom(info: RateLimitInfo | undefined): number | null {
  if (!info) return null;
  const parts: number[] = [];
  if (info.requestsLimit && info.requestsRemaining != null) parts.push(info.requestsRemaining / info.requestsLimit);
  if (info.tokensLimit && info.tokensRemaining != null) parts.push(info.tokensRemaining / info.tokensLimit);
  return parts.length ? Math.max(0, Math.min(1, Math.min(...parts))) : null;
}

export function HeadroomBars({ value }: { value: number | null }) {
  const filled = value == null ? 0 : Math.ceil(value * 4);
  const label = value == null ? "Free headroom unknown" : `Rate limit headroom ${Math.round(value * 100)}%`;
  return (
    <span role="img" aria-label={label} title={label} className="inline-flex items-end gap-px">
      {[0, 1, 2, 3].map((i) => (
        <span
          key={i}
          className={
            value == null
              ? "w-[3px] rounded-sm border border-ink-faint/60"
              : i < filled
                ? "w-[3px] rounded-sm bg-ok"
                : "w-[3px] rounded-sm bg-surface-3"
          }
          style={{ height: 5 + i * 3 }}
        />
      ))}
    </span>
  );
}

export function ModelPicker({ value, onChange }: { value: ModelRef | null; onChange: (m: ModelRef) => void }) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const label = value ? value.modelId : "Choose a model";
  return (
    <div className="relative">
      <button
        ref={button}
        type="button"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className="flex h-8 max-w-72 items-center gap-2 truncate rounded-md border border-line-strong px-3 text-ink-muted hover:bg-surface-3 hover:text-ink"
      >
        <span className="truncate">{label}</span>
      </button>
      {open && (
        <PickerPanel
          value={value}
          onPick={(m) => {
            onChange(m);
            setOpen(false);
            button.current?.focus();
          }}
          onClose={() => {
            setOpen(false);
            button.current?.focus();
          }}
        />
      )}
    </div>
  );
}

export function PickerPanel({
  value,
  onPick,
  onClose,
}: {
  value: ModelRef | null;
  onPick: (m: ModelRef) => void;
  onClose: () => void;
}) {
  const { providers } = useAppData();
  const { settings, update } = useSettings();
  const ready = providers.filter(usable);
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [limits, setLimits] = useState<Record<string, RateLimitInfo>>({});
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SortKey>("cheapest");
  const [freeOnly, setFreeOnly] = useState(false);
  const [filter, setFilter] = useState<string[]>([]);
  const [loadErrors, setLoadErrors] = useState<string[]>([]);
  const panel = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const readyIds = ready.map((p) => p.id).join(",");

  useEffect(() => {
    search.current?.focus();
    const onDown = (e: MouseEvent) => {
      if (!panel.current?.contains(e.target as Node)) onClose();
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [onClose]);

  useEffect(() => {
    let alive = true;
    const ids = readyIds ? readyIds.split(",") : [];
    commands.ratelimits().then((r) => alive && setLimits(r as Record<string, RateLimitInfo>), () => {});
    Promise.all(
      ids.map(async (id) => {
        let list = await commands.modelsList(id).catch(() => [] as ModelEntry[]);
        if (!fetchedLive.has(id) && !list.some((m) => m.live)) {
          fetchedLive.add(id);
          try {
            list = await commands.modelsRefresh(id);
          } catch (e) {
            if (alive) setLoadErrors((errs) => [...errs, `${id}: ${String(e)}`]);
          }
        }
        return list;
      }),
    ).then((lists) => alive && setModels(lists.flat()));
    return () => {
      alive = false;
    };
  }, [readyIds]);

  const favorites = useMemo(() => new Set(settings.favoriteModels), [settings.favoriteModels]);
  const shown = useMemo(
    () => sortModels(filterModels(models, { query, freeOnly, providers: filter }), sort, { favorites, speed: loadSpeeds() }),
    [models, query, freeOnly, filter, sort, favorites],
  );

  function toggleFavorite(key: string) {
    const next = new Set(settings.favoriteModels);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    void update({ favoriteModels: [...next] });
  }

  const listId = "model-list";
  return (
    <div
      ref={panel}
      role="dialog"
      aria-label="Choose a model"
      onKeyDown={(e) => e.key === "Escape" && onClose()}
      className="absolute bottom-10 left-0 z-30 flex h-[28rem] w-[min(46rem,calc(100vw-6rem))] overflow-hidden rounded-xl border border-line-strong bg-surface-2 shadow-2xl"
    >
      <div className="w-14 shrink-0 overflow-y-auto border-r border-line py-2">
        <ProviderRail
          label="Filter by provider"
          selected={filter}
          onToggle={(id) => setFilter((f) => (f.includes(id) ? f.filter((x) => x !== id) : [...f, id]))}
        />
      </div>
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex items-center gap-2 border-b border-line p-2">
          <input
            ref={search}
            type="search"
            aria-label="Search models"
            aria-controls={listId}
            placeholder="Search models"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                panel.current?.querySelector<HTMLElement>("[role=option]")?.focus();
              }
            }}
            className="h-8 min-w-0 flex-1 rounded-md border border-line-strong bg-night px-2"
          />
          <label className="sr-only" htmlFor="model-sort">
            Sort
          </label>
          <select
            id="model-sort"
            value={sort}
            onChange={(e) => setSort(e.target.value as SortKey)}
            className="h-8 rounded-md border border-line-strong bg-night px-2"
          >
            {SORTS.map((s) => (
              <option key={s.id} value={s.id}>
                {s.label}
              </option>
            ))}
          </select>
          <button
            type="button"
            role="switch"
            aria-checked={freeOnly}
            onClick={() => setFreeOnly((f) => !f)}
            className="h-8 rounded-md border border-line-strong px-2 whitespace-nowrap aria-checked:border-ok aria-checked:text-ok"
          >
            Free only
          </button>
        </div>
        {ready.length === 0 ? (
          <p className="p-6 text-ink-muted">No provider is set up yet. Add a key on the Providers page, or start Ollama.</p>
        ) : (
          <ul
            id={listId}
            role="listbox"
            aria-label="Models"
            className="min-h-0 flex-1 overflow-y-auto p-1"
            onKeyDown={(e) => {
              const opts = [...(panel.current?.querySelectorAll<HTMLElement>("[role=option]") ?? [])];
              const i = opts.indexOf(document.activeElement as HTMLElement);
              if (e.key === "ArrowDown") {
                e.preventDefault();
                opts[Math.min(i + 1, opts.length - 1)]?.focus();
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                if (i <= 0) search.current?.focus();
                else opts[i - 1]?.focus();
              }
            }}
          >
            {shown.map((m) => {
              const key = modelKey(m);
              const selected = value?.providerId === m.providerId && value.modelId === m.id;
              const provider = providers.find((p) => p.id === m.providerId);
              return (
                <li
                  key={key}
                  role="option"
                  aria-selected={selected}
                  tabIndex={-1}
                  onClick={() => onPick({ providerId: m.providerId, modelId: m.id })}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      onPick({ providerId: m.providerId, modelId: m.id });
                    }
                  }}
                  className="group flex cursor-pointer items-center gap-3 rounded-md px-2 py-1.5 hover:bg-surface-3 focus:bg-surface-3 aria-selected:bg-surface-3"
                >
                  <button
                    type="button"
                    aria-label={favorites.has(key) ? `Unstar ${m.name}` : `Star ${m.name}`}
                    aria-pressed={favorites.has(key)}
                    onClick={(e) => {
                      e.stopPropagation();
                      toggleFavorite(key);
                    }}
                    className="text-ink-faint hover:text-ink aria-pressed:text-beam"
                  >
                    <IconStar size={15} filled={favorites.has(key)} />
                  </button>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate">{m.name}</span>
                    <span className="block truncate text-xs text-ink-faint">
                      {provider?.name ?? m.providerId} · {m.id}
                    </span>
                  </span>
                  {m.free ? (
                    <span className="rounded bg-ok/15 px-1.5 py-0.5 text-xs font-bold tracking-wide text-ok">FREE</span>
                  ) : m.pricingKnown ? (
                    <span className="text-xs whitespace-nowrap text-ink-muted" title="USD per 1M tokens, in / out">
                      {formatPrice(m.priceIn)} / {formatPrice(m.priceOut)}
                    </span>
                  ) : (
                    <span className="text-xs whitespace-nowrap text-ink-faint">unknown pricing</span>
                  )}
                  <span className="w-10 text-right text-xs text-ink-faint" title="Context window">
                    {formatContext(m.context)}
                  </span>
                  <span className="flex w-9 justify-end gap-1 text-ink-faint">
                    {m.tools && (
                      <span title="Tool calling" aria-label="Tool calling">
                        <IconTool size={14} />
                      </span>
                    )}
                    {m.vision && (
                      <span title="Image input" aria-label="Image input">
                        <IconEye size={14} />
                      </span>
                    )}
                  </span>
                  <HeadroomBars value={headroom(limits[m.providerId])} />
                </li>
              );
            })}
            {shown.length === 0 && <li className="p-4 text-ink-muted">No models match.</li>}
          </ul>
        )}
        {loadErrors.length > 0 && (
          <p className="border-t border-line px-3 py-1.5 text-xs text-ink-faint">
            Couldn't load live lists for: {loadErrors.join("; ")}
          </p>
        )}
      </div>
    </div>
  );
}
