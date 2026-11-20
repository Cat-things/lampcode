import { mockIPC } from "@tauri-apps/api/mocks";
import type { Settings_Serialize } from "../ipc/bindings";

export const DEFAULT_SETTINGS: Settings_Serialize = {
  theme: "dark",
  updateCheck: "disabled",
  providerOrder: [],
  disabledProviders: [],
  favoriteModels: [],
  lastModel: null,
};

type Handler = (cmd: string, args: Record<string, unknown>) => unknown;

/**
 * In-memory fake of the backend. Settings commands are built in; `extra`
 * handles anything else (return `undefined` to fall through to defaults).
 * Returns the list of saved settings values.
 */
export function mockSettings(initial: Partial<Settings_Serialize>, extra?: Handler) {
  let current: Settings_Serialize = { ...DEFAULT_SETTINGS, ...initial };
  const saved: Settings_Serialize[] = [];
  mockIPC((cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    const custom = extra?.(cmd, a);
    if (custom !== undefined) return custom;
    if (cmd === "settings_get") return current;
    if (cmd === "settings_set") {
      current = { ...current, ...(a as { settings: Settings_Serialize }).settings };
      saved.push(current);
      return null;
    }
    if (cmd === "app_info") return { name: "LampCode", version: "0.1.0", dataDir: "/tmp/lc" };
    if (cmd === "keychain_self_test") return { ok: true, message: "ok" };
    if (cmd === "providers_list") return { providers: [], warnings: [], ageLocked: false };
    if (cmd === "sessions_list" || cmd === "projects_list" || cmd === "sessions_search") return [];
    if (cmd === "ratelimits") return {};
    if (cmd === "models_list") return [];
    if (cmd === "catalog_info") return "2026-11-20";
    return null;
  });
  return saved;
}
