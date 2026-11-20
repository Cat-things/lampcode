import { useEffect, useMemo, useRef, useState } from "react";
import { commands } from "../../ipc/bindings";
import { useSettings } from "../../settings/SettingsProvider";
import { useAppData } from "../../state/AppData";

export type PaletteAction = { id: string; label: string; hint?: string; run: () => void };

export const SHORTCUTS: { keys: string; what: string }[] = [
  { keys: "Ctrl+K", what: "Command palette" },
  { keys: "Ctrl+N", what: "New chat" },
  { keys: "Ctrl+Shift+F", what: "Search chats" },
  { keys: "Ctrl+,", what: "Settings" },
  { keys: "Enter / Shift+Enter", what: "Send / new line" },
  { keys: "Esc", what: "Stop the reply (in the composer)" },
  { keys: "Alt+Up / Alt+Down", what: "Move a provider in the rail" },
];

export function CommandPalette({ onClose }: { onClose: () => void }) {
  const { sessions, projects, setView, newChat, reloadSessions } = useAppData();
  const { settings, update } = useSettings();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    input.current?.focus();
  }, []);

  const actions = useMemo<PaletteAction[]>(() => {
    const go = (fn: () => void) => () => {
      onClose();
      fn();
    };
    const base: PaletteAction[] = [
      { id: "new", label: "New chat", hint: "Ctrl+N", run: go(() => newChat(null)) },
      {
        id: "open",
        label: "Open project folder...",
        run: go(async () => {
          const p = await commands.projectPick().catch(() => null);
          if (p) {
            await reloadSessions();
            newChat(p.id);
          }
        }),
      },
      { id: "sessions", label: "Go to Sessions", run: go(() => setView({ kind: "sessions" })) },
      { id: "providers", label: "Go to Providers", run: go(() => setView({ kind: "providers" })) },
      { id: "settings", label: "Go to Settings", hint: "Ctrl+,", run: go(() => setView({ kind: "settings" })) },
      {
        id: "theme",
        label: `Switch to ${settings.theme === "light" ? "dark" : "light"} theme`,
        run: go(() => void update({ theme: settings.theme === "light" ? "dark" : "light" })),
      },
      ...projects.map((p) => ({ id: `p-${p.id}`, label: `New chat in ${p.name}`, run: go(() => newChat(p.id)) })),
      ...sessions.slice(0, 50).map((s) => ({
        id: `s-${s.id}`,
        label: `Open: ${s.title || "Untitled chat"}`,
        run: go(() => setView({ kind: "chat", sessionId: s.id, projectId: s.projectId })),
      })),
      ...SHORTCUTS.map((s) => ({ id: `k-${s.keys}`, label: `Shortcut: ${s.what}`, hint: s.keys, run: () => {} })),
    ];
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return base.filter((a) => words.every((w) => a.label.toLowerCase().includes(w)));
  }, [query, sessions, projects, settings.theme, onClose, newChat, setView, update, reloadSessions]);

  useEffect(() => setActive(0), [query]);

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/40 pt-[15vh]" onMouseDown={onClose}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onMouseDown={(e) => e.stopPropagation()}
        className="w-full max-w-lg overflow-hidden rounded-xl border border-line-strong bg-surface-2 shadow-2xl"
      >
        <input
          ref={input}
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-list"
          aria-activedescendant={actions[active] ? `pal-${actions[active].id}` : undefined}
          aria-label="Type a command"
          placeholder="Type a command or chat title"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") onClose();
            else if (e.key === "ArrowDown") {
              e.preventDefault();
              setActive((a) => Math.min(a + 1, actions.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive((a) => Math.max(a - 1, 0));
            } else if (e.key === "Enter") {
              e.preventDefault();
              actions[active]?.run();
            }
          }}
          className="h-11 w-full border-b border-line bg-transparent px-4 outline-none"
        />
        <ul id="palette-list" role="listbox" aria-label="Commands" className="max-h-80 overflow-y-auto p-1">
          {actions.map((a, i) => (
            <li
              key={a.id}
              id={`pal-${a.id}`}
              role="option"
              aria-selected={i === active}
              onMouseEnter={() => setActive(i)}
              onClick={a.run}
              className="flex cursor-pointer items-center justify-between rounded-md px-3 py-1.5 aria-selected:bg-surface-3"
            >
              <span className="truncate">{a.label}</span>
              {a.hint && <kbd className="ml-3 text-xs text-ink-faint">{a.hint}</kbd>}
            </li>
          ))}
          {actions.length === 0 && <li className="px-3 py-2 text-ink-faint">Nothing matches.</li>}
        </ul>
      </div>
    </div>
  );
}
