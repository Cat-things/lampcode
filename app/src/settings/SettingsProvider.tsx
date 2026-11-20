import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { commands, type Settings_Serialize as Settings, type Theme } from "../ipc/bindings";

type Ctx = {
  settings: Settings;
  update: (patch: Partial<Settings>) => Promise<void>;
};

const SettingsContext = createContext<Ctx | null>(null);

function applyTheme(theme: Theme) {
  const root = document.documentElement;
  const resolved =
    theme === "system"
      ? window.matchMedia?.("(prefers-color-scheme: light)").matches
        ? "light"
        : "dark"
      : theme;
  root.dataset.theme = resolved;
}

/** Loads settings once, then renders children. Saves go straight to the backend. */
export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    commands.settingsGet().then(setSettings, (e: unknown) => setError(String(e)));
  }, []);

  useEffect(() => {
    if (!settings) return;
    applyTheme(settings.theme);
    if (settings.theme !== "system" || !window.matchMedia) return;
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const onChange = () => applyTheme("system");
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [settings]);

  // Latest value, so two quick updates don't overwrite each other.
  const latest = useRef<Settings | null>(null);
  latest.current = settings;

  const update = useCallback(async (patch: Partial<Settings>) => {
    const prev = latest.current;
    if (!prev) return;
    const next = { ...prev, ...patch };
    latest.current = next;
    setSettings(next);
    try {
      await commands.settingsSet(next);
    } catch (e) {
      // Roll back only this patch's keys, keeping later changes.
      const rolled = { ...latest.current, ...Object.fromEntries(Object.keys(patch).map((k) => [k, prev[k as keyof Settings]])) } as Settings;
      latest.current = rolled;
      setSettings(rolled);
      setError(String(e));
    }
  }, []);

  if (error && !settings) {
    return (
      <p role="alert" className="p-8 text-danger">
        Couldn't load settings: {error}
      </p>
    );
  }
  if (!settings) return null;
  return <SettingsContext.Provider value={{ settings, update }}>{children}</SettingsContext.Provider>;
}

export function useSettings(): Ctx {
  const ctx = useContext(SettingsContext);
  if (!ctx) throw new Error("useSettings must be used inside <SettingsProvider>");
  return ctx;
}
