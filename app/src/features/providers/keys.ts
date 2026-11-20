import type { KeyPreview, KeyState } from "../../ipc/bindings";

export function savedKey(k: KeyState): KeyPreview | null {
  return typeof k === "object" && k.set ? k.set : null;
}

export function keyLabel(k: KeyState): string {
  if (k === "notNeeded") return "No key needed";
  if (k === "missing") return "No key";
  if (k === "locked") return "Locked (unlock the key file)";
  const set = savedKey(k);
  if (set) return `${set.masked} (${set.source === "Env" ? "from env var" : set.source === "Keychain" ? "keychain" : "encrypted file"})`;
  return `Error: ${typeof k === "object" ? k.error : k}`;
}
