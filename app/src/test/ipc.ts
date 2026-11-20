import { mockIPC } from "@tauri-apps/api/mocks";
import type { Settings_Serialize } from "../ipc/bindings";

/** In-memory fake of the settings commands. Returns the list of saved values. */
export function mockSettings(initial: Settings_Serialize) {
  let current = { ...initial };
  const saved: Settings_Serialize[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "settings_get") return current;
    if (cmd === "settings_set") {
      current = { ...current, ...(args as { settings: Settings_Serialize }).settings };
      saved.push(current);
      return null;
    }
    if (cmd === "app_info") return { name: "LampCode", version: "0.1.0", dataDir: "/tmp/lc" };
    if (cmd === "keychain_self_test") return { ok: true, message: "ok" };
  });
  return saved;
}
