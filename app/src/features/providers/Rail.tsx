import { useEffect, useRef, useState } from "react";
import { commands, type ProviderView_Serialize as ProviderView } from "../../ipc/bindings";
import { errorText } from "../../lib/format";
import { useSettings } from "../../settings/SettingsProvider";
import { orderProviders, useAppData } from "../../state/AppData";
import { KeyDialog } from "./KeyDialog";
import { savedKey } from "./keys";
import { ProviderIcon } from "./ProviderIcon";

type Menu = { provider: ProviderView; x: number; y: number };

export function usable(p: ProviderView) {
  return p.configured && !p.disabled;
}

/**
 * Vertical provider icon rail. Click filters, drag (or Alt+Up/Down) reorders,
 * right-click (or the context-menu key) opens key/edit/remove/disable actions.
 * Greyed out = not configured or disabled.
 */
export function ProviderRail({
  selected,
  onToggle,
  label = "Providers",
}: {
  selected: string[];
  onToggle: (id: string) => void;
  label?: string;
}) {
  const { providers, reloadProviders, setView } = useAppData();
  const { settings, update } = useSettings();
  const ordered = orderProviders(providers, settings.providerOrder);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [keyFor, setKeyFor] = useState<ProviderView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const dragId = useRef<string | null>(null);

  function move(id: string, to: number) {
    const ids = ordered.map((p) => p.id).filter((x) => x !== id);
    ids.splice(Math.max(0, Math.min(to, ids.length)), 0, id);
    void update({ providerOrder: ids });
  }

  async function act(fn: () => Promise<unknown>) {
    setMenu(null);
    setError(null);
    try {
      await fn();
      await reloadProviders();
    } catch (e) {
      setError(errorText(e));
    }
  }

  function toggleDisabled(p: ProviderView) {
    const set = new Set(settings.disabledProviders);
    if (set.has(p.id)) set.delete(p.id);
    else set.add(p.id);
    return update({ disabledProviders: [...set] });
  }

  return (
    <div className="flex flex-col items-center gap-1.5">
      <ul aria-label={label} className="flex flex-col items-center gap-1.5">
        {ordered.map((p, i) => {
          const on = selected.includes(p.id);
          const state = p.disabled ? "disabled" : p.configured ? "ready" : "not configured";
          return (
            <li
              key={p.id}
              draggable
              onDragStart={() => (dragId.current = p.id)}
              onDragOver={(e) => e.preventDefault()}
              onDrop={(e) => {
                e.preventDefault();
                if (dragId.current && dragId.current !== p.id) move(dragId.current, i);
                dragId.current = null;
              }}
            >
              <button
                type="button"
                aria-pressed={on}
                aria-label={`${p.name} (${state})`}
                title={`${p.name}: ${state}. Right-click for options.`}
                onClick={() => onToggle(p.id)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenu({ provider: p, x: e.clientX, y: e.clientY });
                }}
                onKeyDown={(e) => {
                  if (e.altKey && e.key === "ArrowUp") {
                    e.preventDefault();
                    move(p.id, i - 1);
                  } else if (e.altKey && e.key === "ArrowDown") {
                    e.preventDefault();
                    move(p.id, i + 1);
                  } else if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
                    e.preventDefault();
                    const r = e.currentTarget.getBoundingClientRect();
                    setMenu({ provider: p, x: r.right, y: r.top });
                  }
                }}
                className="rounded-lg p-1 ring-beam aria-pressed:ring-2 hover:bg-surface-3"
              >
                <ProviderIcon id={p.id} name={p.name} dim={!usable(p)} />
              </button>
            </li>
          );
        })}
      </ul>
      {error && (
        <p role="alert" className="max-w-40 text-xs text-danger">
          {error}
        </p>
      )}
      {menu && (
        <ContextMenu
          menu={menu}
          onClose={() => setMenu(null)}
          items={[
            ...(menu.provider.key === "notNeeded"
              ? []
              : [
                  {
                    label: menu.provider.key === "missing" ? "Add key..." : "Replace key...",
                    run: () => {
                      setMenu(null);
                      setKeyFor(menu.provider);
                    },
                  },
                ]),
            {
              label: "Edit...",
              run: () => {
                setMenu(null);
                setView({ kind: "providers", providerId: menu.provider.id });
              },
            },
            ...(savedKey(menu.provider.key) && savedKey(menu.provider.key)?.source !== "Env"
              ? [{ label: "Remove key", run: () => act(() => commands.keyDelete(menu.provider.id)) }]
              : []),
            {
              label: menu.provider.disabled ? "Enable" : "Disable",
              run: () => act(() => toggleDisabled(menu.provider)),
            },
          ]}
        />
      )}
      {keyFor && <KeyDialog provider={keyFor} onClose={() => setKeyFor(null)} onSaved={() => void reloadProviders()} />}
    </div>
  );
}

function ContextMenu({ menu, items, onClose }: { menu: Menu; items: { label: string; run: () => void }[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>("button")?.focus();
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [onClose]);
  return (
    <div
      ref={ref}
      role="menu"
      aria-label={`${menu.provider.name} options`}
      style={{ left: menu.x, top: menu.y }}
      onKeyDown={(e) => {
        const btns = [...(ref.current?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
        const i = btns.indexOf(document.activeElement as HTMLButtonElement);
        if (e.key === "Escape") onClose();
        if (e.key === "ArrowDown") btns[(i + 1) % btns.length]?.focus();
        if (e.key === "ArrowUp") btns[(i - 1 + btns.length) % btns.length]?.focus();
      }}
      className="fixed z-50 min-w-40 rounded-lg border border-line-strong bg-surface-2 p-1 shadow-xl"
    >
      {items.map((it) => (
        <button
          key={it.label}
          role="menuitem"
          type="button"
          onClick={it.run}
          className="block w-full rounded-md px-3 py-1.5 text-left hover:bg-surface-3 focus:bg-surface-3"
        >
          {it.label}
        </button>
      ))}
    </div>
  );
}
