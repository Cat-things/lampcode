import { useState } from "react";
import { commands, type KeychainSelfTest } from "../../ipc/bindings";

export function KeychainTest() {
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<KeychainSelfTest | null>(null);

  async function run() {
    setRunning(true);
    try {
      setResult(await commands.keychainSelfTest());
    } catch (e) {
      setResult({ ok: false, message: String(e) });
    } finally {
      setRunning(false);
    }
  }

  return (
    <div className="flex flex-wrap items-center gap-3">
      <button
        type="button"
        onClick={run}
        disabled={running}
        className="h-8 rounded-md border border-line-strong px-3 text-ink hover:bg-surface-3 disabled:opacity-60"
      >
        Test keychain
      </button>
      <p role="status" className={result ? (result.ok ? "text-ok" : "text-danger") : "text-ink-faint"}>
        {running ? "Testing…" : result ? result.message : "Writes, reads and deletes a throwaway value. Your saved keys are not touched."}
      </p>
    </div>
  );
}
