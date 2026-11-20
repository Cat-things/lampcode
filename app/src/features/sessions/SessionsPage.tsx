import { useState } from "react";
import { commands } from "../../ipc/bindings";
import { errorText } from "../../lib/format";
import { useAppData } from "../../state/AppData";
import { useSessionSearch } from "./Sidebar";

export function SessionsPage() {
  const { sessions, projects, setView, reloadSessions } = useAppData();
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const hits = useSessionSearch(query);
  const projectName = (id: string | null) => projects.find((p) => p.id === id)?.name;
  const rows = hits
    ? hits.map((h) => ({ id: h.sessionId, title: h.title, sub: h.snippet, s: sessions.find((s) => s.id === h.sessionId) }))
    : sessions.map((s) => ({
        id: s.id,
        title: s.title,
        sub: `${projectName(s.projectId) ?? "No project"} · ${new Date(s.updatedAt ?? 0).toLocaleString()}`,
        s,
      }));

  return (
    <div className="h-full overflow-y-auto p-8">
      <h1 className="text-xl font-semibold">Sessions</h1>
      <p className="mt-1 text-ink-muted">Every chat is stored only on this computer.</p>
      <input
        type="search"
        aria-label="Search all sessions"
        placeholder="Search all sessions"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        className="mt-5 h-9 w-full max-w-xl rounded-md border border-line-strong bg-night px-3"
      />
      {error && (
        <p role="alert" className="mt-2 text-danger">
          {error}
        </p>
      )}
      <ul className="mt-4 flex max-w-3xl flex-col divide-y divide-line">
        {rows.length === 0 && <li className="py-3 text-ink-faint">{hits ? "No matches." : "No sessions yet."}</li>}
        {rows.map((r) => (
          <li key={r.id} className="flex items-center gap-3 py-2">
            <button
              type="button"
              onClick={() => setView({ kind: "chat", sessionId: r.id, projectId: r.s?.projectId ?? null })}
              className="min-w-0 flex-1 rounded-md px-2 py-1 text-left hover:bg-surface-2"
            >
              <span className="block truncate">{r.title || "Untitled chat"}</span>
              <span className="block truncate text-xs text-ink-faint">{r.sub}</span>
            </button>
            <button
              type="button"
              onClick={async () => {
                try {
                  await commands.sessionDelete(r.id);
                  await reloadSessions();
                } catch (e) {
                  setError(errorText(e));
                }
              }}
              className="h-7 rounded-md border border-line px-2 text-xs text-ink-muted hover:border-danger hover:text-danger"
            >
              Delete
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
