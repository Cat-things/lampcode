import { useEffect, useState } from "react";
import { commands, type AppInfo } from "../../ipc/bindings";

type State = { kind: "loading" } | { kind: "ready"; info: AppInfo } | { kind: "error"; message: string };

export function About() {
  const [state, setState] = useState<State>({ kind: "loading" });

  useEffect(() => {
    let live = true;
    commands
      .appInfo()
      .then((info) => live && setState({ kind: "ready", info }))
      .catch((e: unknown) => live && setState({ kind: "error", message: String(e) }));
    return () => {
      live = false;
    };
  }, []);

  return (
    <div>
      {state.kind === "loading" && <p className="text-ink-faint">Loading…</p>}
      {state.kind === "error" && (
        <p role="alert" className="text-danger">
          Couldn't read app info: {state.message}
        </p>
      )}
      {state.kind === "ready" && (
        <dl className="grid grid-cols-[8rem_1fr] gap-y-2 rounded-lg border border-line bg-surface-2 p-4">
          <dt className="text-ink-muted">Version</dt>
          <dd>{state.info.version}</dd>
          <dt className="text-ink-muted">Data folder</dt>
          <dd className="break-all font-mono text-[13px]">{state.info.dataDir}</dd>
        </dl>
      )}
    </div>
  );
}
