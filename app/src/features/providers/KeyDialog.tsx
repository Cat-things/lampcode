import { useEffect, useRef, useState } from "react";
import { commands, type ProviderView_Serialize as ProviderView } from "../../ipc/bindings";
import { errorText } from "../../lib/format";

/** Paste a key once. It goes straight to the backend key store and is never shown again. */
export function KeyDialog({ provider, onClose, onSaved }: { provider: ProviderView; onClose: () => void; onSaved: () => void }) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    input.current?.focus();
  }, []);

  async function save(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await commands.keySet(provider.id, key);
      setKey("");
      onSaved();
      onClose();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/50 p-4" onMouseDown={onClose}>
      <form
        role="dialog"
        aria-modal="true"
        aria-labelledby="key-dialog-title"
        onSubmit={save}
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
        className="w-full max-w-md rounded-xl border border-line-strong bg-surface-2 p-5 shadow-2xl"
      >
        <h2 id="key-dialog-title" className="text-base font-semibold">
          API key for {provider.name}
        </h2>
        <p className="mt-1 text-ink-muted">
          Stored in your OS keychain. LampCode never shows it again, only a short preview.
          {provider.keyEnv && (
            <>
              {" "}
              Or set <code className="text-ink">{provider.keyEnv}</code> and skip saving it at all.
            </>
          )}
        </p>
        {provider.signupUrl && (
          <p className="mt-2 text-ink-faint">
            Get a key at <span className="select-all text-ink-muted">{provider.signupUrl}</span>
          </p>
        )}
        <label className="mt-4 block text-ink-muted" htmlFor="key-input">
          Key
        </label>
        <input
          id="key-input"
          ref={input}
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={key}
          onChange={(e) => setKey(e.target.value)}
          className="mt-1 h-9 w-full rounded-md border border-line-strong bg-night px-3 font-mono text-ink"
        />
        {error && (
          <p role="alert" className="mt-2 text-danger">
            {error}
          </p>
        )}
        <div className="mt-5 flex justify-end gap-2">
          <button type="button" onClick={onClose} className="h-8 rounded-md px-3 text-ink-muted hover:bg-surface-3">
            Cancel
          </button>
          <button
            type="submit"
            disabled={busy || key.trim() === ""}
            className="h-8 rounded-md bg-beam px-3 font-medium text-beam-ink disabled:opacity-50"
          >
            Save key
          </button>
        </div>
      </form>
    </div>
  );
}
