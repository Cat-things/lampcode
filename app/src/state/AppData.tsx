import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import {
  commands,
  type ModelRef,
  type Project,
  type ProviderView_Serialize as ProviderView,
  type ProvidersState_Serialize as ProvidersState,
  type SessionSummary_Serialize as SessionSummary,
} from "../ipc/bindings";
import { errorText } from "../lib/format";

export type View =
  | { kind: "chat"; sessionId: string | null; projectId: string | null }
  | { kind: "sessions" }
  | { kind: "providers"; providerId?: string }
  | { kind: "settings" };

type Ctx = {
  providers: ProviderView[];
  providerWarnings: string[];
  ageLocked: boolean;
  sessions: SessionSummary[];
  projects: Project[];
  error: string | null;
  view: View;
  setView: (v: View) => void;
  reloadProviders: () => Promise<void>;
  reloadSessions: () => Promise<void>;
  newChat: (projectId?: string | null) => void;
  /** Default model for new chats: last used, else nothing. */
  defaultModel: ModelRef | null;
};

const AppDataContext = createContext<Ctx | null>(null);

export function AppDataProvider({ children, lastModel }: { children: ReactNode; lastModel: ModelRef | null }) {
  const [providers, setProviders] = useState<ProvidersState>({ providers: [], warnings: [], ageLocked: false });
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>({ kind: "chat", sessionId: null, projectId: null });

  const reloadProviders = useCallback(async () => {
    try {
      setProviders(await commands.providersList());
    } catch (e) {
      setError(errorText(e));
    }
  }, []);

  const reloadSessions = useCallback(async () => {
    try {
      const [s, p] = await Promise.all([commands.sessionsList(), commands.projectsList()]);
      setSessions(s);
      setProjects(p);
    } catch (e) {
      setError(errorText(e));
    }
  }, []);

  useEffect(() => {
    void reloadProviders();
    void reloadSessions();
  }, [reloadProviders, reloadSessions]);

  const newChat = useCallback((projectId: string | null = null) => {
    setView({ kind: "chat", sessionId: null, projectId });
  }, []);

  return (
    <AppDataContext.Provider
      value={{
        providers: providers.providers,
        providerWarnings: providers.warnings,
        ageLocked: providers.ageLocked,
        sessions,
        projects,
        error,
        view,
        setView,
        reloadProviders,
        reloadSessions,
        newChat,
        defaultModel: lastModel,
      }}
    >
      {children}
    </AppDataContext.Provider>
  );
}

export function useAppData(): Ctx {
  const ctx = useContext(AppDataContext);
  if (!ctx) throw new Error("useAppData must be used inside <AppDataProvider>");
  return ctx;
}

/** Providers in the user's rail order: saved order first, then the rest by name. */
export function orderProviders(list: ProviderView[], order: string[]): ProviderView[] {
  const pos = new Map(order.map((id, i) => [id, i]));
  return [...list].sort((a, b) => {
    const pa = pos.get(a.id) ?? Number.MAX_SAFE_INTEGER;
    const pb = pos.get(b.id) ?? Number.MAX_SAFE_INTEGER;
    return pa !== pb ? pa - pb : a.name.localeCompare(b.name);
  });
}
