import { useEffect, useState } from "react";
import { Chat } from "./features/chat/Chat";
import { UpdatePrompt } from "./features/onboarding/UpdatePrompt";
import { CommandPalette } from "./features/palette/CommandPalette";
import { ProvidersPage } from "./features/providers/ProvidersPage";
import { SessionsPage } from "./features/sessions/SessionsPage";
import { Sidebar } from "./features/sessions/Sidebar";
import { SettingsPage } from "./features/settings/SettingsPage";
import { IconChat, IconGear, IconPlug, IconStack } from "./lib/icons";
import { SettingsProvider, useSettings } from "./settings/SettingsProvider";
import { AppDataProvider, useAppData, type View } from "./state/AppData";

const RAIL: { kind: View["kind"]; label: string; icon: typeof IconChat }[] = [
  { kind: "chat", label: "Home", icon: IconChat },
  { kind: "sessions", label: "Sessions", icon: IconStack },
  { kind: "providers", label: "Providers", icon: IconPlug },
  { kind: "settings", label: "Settings", icon: IconGear },
];

function Shell() {
  const { view, setView, newChat } = useAppData();
  const [palette, setPalette] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (!mod) return;
      const k = e.key.toLowerCase();
      if (k === "k") {
        e.preventDefault();
        setPalette((p) => !p);
      } else if (k === "n" && !e.shiftKey) {
        e.preventDefault();
        newChat(view.kind === "chat" ? view.projectId : null);
      } else if (k === ",") {
        e.preventDefault();
        setView({ kind: "settings" });
      } else if (k === "f" && e.shiftKey) {
        e.preventDefault();
        if (view.kind !== "chat") setView({ kind: "chat", sessionId: null, projectId: null });
        window.setTimeout(() => document.getElementById("sidebar-search")?.focus(), 0);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [view, setView, newChat]);

  return (
    <div className="flex h-full bg-night text-ink">
      <nav aria-label="Main" className="flex w-14 shrink-0 flex-col items-center gap-1 border-r border-line bg-surface-1 py-2">
        {RAIL.map((r) => (
          <button
            key={r.kind}
            type="button"
            aria-label={r.label}
            title={r.label}
            aria-current={view.kind === r.kind ? "page" : undefined}
            onClick={() => (r.kind === "chat" ? view.kind !== "chat" && newChat(null) : setView({ kind: r.kind } as View))}
            className="flex h-10 w-10 items-center justify-center rounded-lg text-ink-muted hover:bg-surface-2 hover:text-ink aria-[current=page]:bg-surface-3 aria-[current=page]:text-ink"
          >
            <r.icon size={20} />
          </button>
        ))}
      </nav>
      {view.kind === "chat" && <Sidebar />}
      <main className="min-w-0 flex-1 overflow-hidden">
        {view.kind === "chat" && <Chat sessionId={view.sessionId} projectId={view.projectId} />}
        {view.kind === "sessions" && <SessionsPage />}
        {view.kind === "providers" && <ProvidersPage initial={view.providerId} />}
        {view.kind === "settings" && (
          <div className="h-full overflow-y-auto">
            <SettingsPage />
          </div>
        )}
      </main>
      {palette && <CommandPalette onClose={() => setPalette(false)} />}
    </div>
  );
}

function WithData() {
  const { settings } = useSettings();
  return (
    <AppDataProvider lastModel={settings.lastModel}>
      <Shell />
    </AppDataProvider>
  );
}

export default function App() {
  return (
    <SettingsProvider>
      <WithData />
      <UpdatePrompt />
    </SettingsProvider>
  );
}
