import { useEffect, useState } from "react";
import { commands, type FreeTier, type ProviderView_Serialize as ProviderView } from "../../ipc/bindings";
import { keyLabel, savedKey } from "./keys";
import { errorText } from "../../lib/format";
import { useSettings } from "../../settings/SettingsProvider";
import { orderProviders, useAppData } from "../../state/AppData";
import { KeyDialog } from "./KeyDialog";
import { ProviderIcon } from "./ProviderIcon";
import { ProviderRail } from "./Rail";

const FREE_LABEL: Record<FreeTier, string> = {
  none: "Paid only",
  free_tier: "Free tier",
  free_models: "Free models",
  trial_credits: "Trial credits",
  local: "Local",
  unknown: "Free tier not stated",
};

export function ProvidersPage({ initial }: { initial?: string }) {
  const { providers, providerWarnings, ageLocked } = useAppData();
  const { settings } = useSettings();
  const ordered = orderProviders(providers, settings.providerOrder);
  const [selected, setSelected] = useState<string | undefined>(initial ?? ordered[0]?.id);
  useEffect(() => {
    if (initial) setSelected(initial);
  }, [initial]);
  useEffect(() => {
    if (!selected && ordered[0]) setSelected(ordered[0].id);
  }, [selected, ordered]);
  const current = providers.find((p) => p.id === selected);

  return (
    <div className="flex h-full min-h-0">
      <div className="w-14 shrink-0 overflow-y-auto border-r border-line py-3">
        <ProviderRail selected={selected ? [selected] : []} onToggle={setSelected} />
      </div>
      <div className="min-w-0 flex-1 overflow-y-auto p-8">
        <h1 className="text-xl font-semibold">Providers</h1>
        <p className="mt-1 max-w-2xl text-ink-muted">
          LampCode only connects to providers you've set up. Drag icons to reorder, right-click for quick actions.
        </p>
        {ageLocked && <AgeUnlock />}
        {providerWarnings.length > 0 && (
          <div role="status" className="mt-4 rounded-lg border border-line-strong bg-surface-1 p-3 text-ink-muted">
            <p className="font-medium text-ink">Some provider packs were skipped</p>
            <ul className="mt-1 list-disc pl-5">
              {providerWarnings.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          </div>
        )}
        {current && <ProviderDetail provider={current} />}
        <CatalogSection />
      </div>
    </div>
  );
}

function ProviderDetail({ provider: p }: { provider: ProviderView }) {
  const { reloadProviders } = useAppData();
  const { settings, update } = useSettings();
  const [keyOpen, setKeyOpen] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function run(label: string, fn: () => Promise<string | void>) {
    setBusy(true);
    setError(null);
    setStatus(`${label}...`);
    try {
      const msg = await fn();
      setStatus(msg ?? null);
      await reloadProviders();
    } catch (e) {
      setStatus(null);
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  const saved = savedKey(p.key);
  const hasSavedKey = !!saved && saved.source !== "Env";
  const disabled = settings.disabledProviders.includes(p.id);

  return (
    <section aria-labelledby="provider-name" className="mt-6 max-w-2xl rounded-xl border border-line bg-surface-1 p-5">
      <div className="flex items-center gap-3">
        <ProviderIcon id={p.id} name={p.name} size={36} dim={!p.configured || p.disabled} />
        <div>
          <h2 id="provider-name" className="text-lg font-semibold">
            {p.name}
          </h2>
          <p className="text-ink-faint">
            {FREE_LABEL[p.freeTier]} · {p.region}
            {p.tags.includes("eu") && " · EU hosted"}
            {p.userPack && " · from a provider pack"}
          </p>
        </div>
      </div>
      {p.freeTierNote && <p className="mt-3 text-ink-muted">{p.freeTierNote}</p>}
      <dl className="mt-4 grid grid-cols-[9rem_1fr] gap-x-4 gap-y-1.5">
        <dt className="text-ink-faint">Key</dt>
        <dd>{keyLabel(p.key)}</dd>
        {p.keyEnv && (
          <>
            <dt className="text-ink-faint">Env var</dt>
            <dd>
              <code>{p.keyEnv}</code> (overrides a saved key)
            </dd>
          </>
        )}
        <dt className="text-ink-faint">Endpoint</dt>
        <dd className="break-all font-mono text-[13px]">{p.baseUrl}</dd>
        <dt className="text-ink-faint">Docs</dt>
        <dd className="break-all text-[13px] select-all">{p.docsUrl}</dd>
        <dt className="text-ink-faint">Checked</dt>
        <dd>{p.checked}</dd>
      </dl>
      <div className="mt-5 flex flex-wrap gap-2">
        {p.key !== "notNeeded" && (
          <button type="button" onClick={() => setKeyOpen(true)} className="h-8 rounded-md bg-beam px-3 font-medium text-beam-ink">
            {p.key === "missing" ? "Add key" : "Replace key"}
          </button>
        )}
        {hasSavedKey && (
          <button
            type="button"
            disabled={busy}
            onClick={() => run("Removing key", async () => void (await commands.keyDelete(p.id)))}
            className="h-8 rounded-md border border-line-strong px-3 hover:bg-surface-3"
          >
            Remove key
          </button>
        )}
        <button
          type="button"
          disabled={busy || !p.configured || disabled}
          onClick={() =>
            run("Loading models", async () => {
              const m = await commands.modelsRefresh(p.id);
              return `Loaded ${m.length} models from ${p.name}.`;
            })
          }
          className="h-8 rounded-md border border-line-strong px-3 hover:bg-surface-3 disabled:opacity-50"
        >
          Load model list
        </button>
        <button
          type="button"
          role="switch"
          aria-checked={!disabled}
          onClick={() => {
            const set = new Set(settings.disabledProviders);
            if (disabled) set.delete(p.id);
            else set.add(p.id);
            void update({ disabledProviders: [...set] }).then(reloadProviders);
          }}
          className="h-8 rounded-md border border-line-strong px-3 hover:bg-surface-3"
        >
          {disabled ? "Disabled (click to enable)" : "Enabled"}
        </button>
      </div>
      {p.protocol === "ollama" && <OllamaModels busy={busy} run={run} />}
      <div aria-live="polite" className="mt-3 min-h-5 text-ink-muted">
        {status}
      </div>
      {error && (
        <p role="alert" className="text-danger">
          {error}
        </p>
      )}
      {keyOpen && <KeyDialog provider={p} onClose={() => setKeyOpen(false)} onSaved={() => void reloadProviders()} />}
    </section>
  );
}

function OllamaModels({ busy, run }: { busy: boolean; run: (label: string, fn: () => Promise<string | void>) => Promise<void> }) {
  const [name, setName] = useState("");
  return (
    <form
      className="mt-4 flex items-end gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        void run(`Downloading ${name}`, async () => {
          await commands.ollamaPull(name.trim());
          return `Downloaded ${name}.`;
        });
      }}
    >
      <label className="flex flex-col gap-1">
        <span className="text-ink-faint">Model name</span>
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="e.g. qwen3:8b"
          className="h-8 w-56 rounded-md border border-line-strong bg-night px-2 font-mono"
        />
      </label>
      <button type="submit" disabled={busy || !name.trim()} className="h-8 rounded-md border border-line-strong px-3 disabled:opacity-50">
        Pull
      </button>
      <button
        type="button"
        disabled={busy || !name.trim()}
        onClick={() =>
          run(`Deleting ${name}`, async () => {
            await commands.ollamaDelete(name.trim());
            return `Deleted ${name}.`;
          })
        }
        className="h-8 rounded-md border border-line-strong px-3 disabled:opacity-50"
      >
        Delete
      </button>
    </form>
  );
}

function AgeUnlock() {
  const { reloadProviders } = useAppData();
  const [pass, setPass] = useState("");
  const [error, setError] = useState<string | null>(null);
  return (
    <form
      aria-labelledby="age-title"
      className="mt-4 max-w-2xl rounded-xl border border-line-strong bg-surface-1 p-4"
      onSubmit={async (e) => {
        e.preventDefault();
        setError(null);
        try {
          await commands.ageUnlock(pass);
          setPass("");
          await reloadProviders();
        } catch (err) {
          setError(errorText(err));
        }
      }}
    >
      <h2 id="age-title" className="font-semibold">
        Unlock your keys
      </h2>
      <p className="mt-1 text-ink-muted">
        No OS keychain is available, so keys are kept in an encrypted file. Enter its passphrase. It stays in memory until
        you quit.
      </p>
      <div className="mt-3 flex gap-2">
        <input
          type="password"
          aria-label="Passphrase"
          value={pass}
          onChange={(e) => setPass(e.target.value)}
          className="h-8 flex-1 rounded-md border border-line-strong bg-night px-2"
        />
        <button type="submit" disabled={!pass} className="h-8 rounded-md bg-beam px-3 font-medium text-beam-ink disabled:opacity-50">
          Unlock
        </button>
      </div>
      {error && (
        <p role="alert" className="mt-2 text-danger">
          {error}
        </p>
      )}
    </form>
  );
}

function CatalogSection() {
  const [date, setDate] = useState<string>("");
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    commands.catalogInfo().then(setDate, () => {});
  }, []);
  return (
    <section aria-labelledby="catalog-title" className="mt-8 max-w-2xl">
      <h2 id="catalog-title" className="font-semibold">
        Model catalog
      </h2>
      <p className="mt-1 text-ink-muted">
        Prices and context sizes come from a bundled models.dev snapshot{date && ` (${date})`}. Refreshing downloads it
        once from models.dev. Nothing else is sent.
      </p>
      <button
        type="button"
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          setMsg(null);
          try {
            const d = await commands.catalogRefresh();
            setDate(d);
            setMsg("Catalog updated.");
          } catch (e) {
            setMsg(`Couldn't refresh: ${errorText(e)}`);
          } finally {
            setBusy(false);
          }
        }}
        className="mt-3 h-8 rounded-md border border-line-strong px-3 hover:bg-surface-3 disabled:opacity-50"
      >
        Refresh from models.dev
      </button>
      <p aria-live="polite" className="mt-2 text-ink-muted">
        {msg}
      </p>
    </section>
  );
}
