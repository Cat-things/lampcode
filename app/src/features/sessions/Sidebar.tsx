import { useEffect, useState } from "react";
import { commands, type SearchHit, type SessionSummary_Serialize as SessionSummary } from "../../ipc/bindings";
import { errorText } from "../../lib/format";
import { IconFolder, IconPlus, IconSearch, IconTrash } from "../../lib/icons";
import { useAppData } from "../../state/AppData";

export function useSessionSearch(query: string) {
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  useEffect(() => {
    if (!query.trim()) {
      setHits(null);
      return;
    }
    let alive = true;
    const t = window.setTimeout(() => {
      commands.sessionsSearch(query).then(
        (h) => alive && setHits(h),
        () => alive && setHits([]),
      );
    }, 150);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
  }, [query]);
  return hits;
}

/** Projects with their sessions nested underneath, plus loose chats. */
export function Sidebar() {
  const { projects, sessions, view, setView, newChat, reloadSessions } = useAppData();
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const hits = useSessionSearch(query);
  const current = view.kind === "chat" ? view.sessionId : null;

  async function openFolder() {
    setError(null);
    try {
      const p = await commands.projectPick();
      if (p) {
        await reloadSessions();
        newChat(p.id);
      }
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function remove(s: SessionSummary) {
    setError(null);
    try {
      await commands.sessionDelete(s.id);
      if (current === s.id) newChat(s.projectId);
      await reloadSessions();
    } catch (e) {
      setError(errorText(e));
    }
  }

  const loose = sessions.filter((s) => !s.projectId || !projects.some((p) => p.id === s.projectId));

  const item = (s: SessionSummary) => (
    <li key={s.id} className="group flex items-center">
      <button
        type="button"
        aria-current={current === s.id ? "page" : undefined}
        onClick={() => setView({ kind: "chat", sessionId: s.id, projectId: s.projectId })}
        className="min-w-0 flex-1 truncate rounded-md px-2 py-1 text-left text-ink-muted hover:bg-surface-2 hover:text-ink aria-[current=page]:bg-surface-3 aria-[current=page]:text-ink"
      >
        {s.title || "Untitled chat"}
      </button>
      <button
        type="button"
        aria-label={`Delete chat ${s.title || "Untitled chat"}`}
        onClick={() => void remove(s)}
        className="ml-0.5 rounded p-1 text-ink-faint opacity-0 group-hover:opacity-100 hover:text-danger focus-visible:opacity-100"
      >
        <IconTrash size={14} />
      </button>
    </li>
  );

  return (
    <aside aria-label="Projects and chats" className="flex w-64 shrink-0 flex-col border-r border-line bg-surface-1">
      <div className="flex gap-1.5 p-2">
        <button
          type="button"
          onClick={() => newChat(view.kind === "chat" ? view.projectId : null)}
          className="flex h-8 flex-1 items-center gap-2 rounded-md bg-surface-3 px-2.5 hover:text-ink"
        >
          <IconPlus size={16} /> New chat
        </button>
        <button
          type="button"
          onClick={openFolder}
          aria-label="Open project folder"
          title="Open project folder"
          className="flex h-8 w-8 items-center justify-center rounded-md border border-line-strong text-ink-muted hover:text-ink"
        >
          <IconFolder size={16} />
        </button>
      </div>
      <div className="relative px-2">
        <IconSearch size={14} className="pointer-events-none absolute top-2.5 left-4 text-ink-faint" />
        <input
          id="sidebar-search"
          type="search"
          aria-label="Search chats"
          placeholder="Search chats"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="h-8 w-full rounded-md border border-line bg-night pr-2 pl-7"
        />
      </div>
      {error && (
        <p role="alert" className="px-3 pt-2 text-xs text-danger">
          {error}
        </p>
      )}
      <nav aria-label="Chats" className="min-h-0 flex-1 overflow-y-auto p-2">
        {hits ? (
          <ul aria-label="Search results" className="flex flex-col gap-0.5">
            {hits.length === 0 && <li className="px-2 py-1 text-ink-faint">No matches.</li>}
            {hits.map((h) => (
              <li key={h.sessionId}>
                <button
                  type="button"
                  onClick={() => {
                    const s = sessions.find((x) => x.id === h.sessionId);
                    setView({ kind: "chat", sessionId: h.sessionId, projectId: s?.projectId ?? null });
                  }}
                  className="w-full rounded-md px-2 py-1 text-left hover:bg-surface-2"
                >
                  <span className="block truncate">{h.title || "Untitled chat"}</span>
                  <span className="block truncate text-xs text-ink-faint">{h.snippet}</span>
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <>
            {projects.map((p) => (
              <section key={p.id} aria-label={`Project ${p.name}`} className="mb-3">
                <div className="group flex items-center gap-1.5 px-2 py-1">
                  <IconFolder size={14} className="text-ink-faint" />
                  <span className="min-w-0 flex-1 truncate font-medium" title={p.path}>
                    {p.name}
                  </span>
                  <button
                    type="button"
                    aria-label={`New chat in ${p.name}`}
                    onClick={() => newChat(p.id)}
                    className="rounded p-0.5 text-ink-faint opacity-0 group-hover:opacity-100 hover:text-ink focus-visible:opacity-100"
                  >
                    <IconPlus size={14} />
                  </button>
                </div>
                <ul className="ml-3 flex flex-col gap-0.5 border-l border-line pl-1.5">
                  {sessions.filter((s) => s.projectId === p.id).map(item)}
                </ul>
              </section>
            ))}
            {loose.length > 0 && (
              <section aria-label="Chats without a project">
                {projects.length > 0 && <p className="px-2 py-1 text-xs text-ink-faint">Other chats</p>}
                <ul className="flex flex-col gap-0.5">{loose.map(item)}</ul>
              </section>
            )}
            {sessions.length === 0 && projects.length === 0 && (
              <p className="px-2 py-1 text-ink-faint">No chats yet. Open a project folder or just start typing.</p>
            )}
          </>
        )}
      </nav>
    </aside>
  );
}
