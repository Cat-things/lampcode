import { useEffect, useState } from "react";
import { commands, type NetworkStatus } from "../../ipc/bindings";
import { errorText } from "../../lib/format";

/** Settings > Privacy > Network: every host LampCode may contact, and what it refused. */
export function NetworkList() {
  const [status, setStatus] = useState<NetworkStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    commands.networkStatus().then(setStatus, (e) => setError(errorText(e)));
  }, []);
  if (error)
    return (
      <p role="alert" className="text-danger">
        {error}
      </p>
    );
  if (!status) return null;
  return (
    <div>
      <h3 className="font-medium">Allowed hosts</h3>
      {status.allowed.length === 0 ? (
        <p className="text-ink-faint">None. Nothing leaves this computer until you set up a provider.</p>
      ) : (
        <ul className="mt-1 font-mono text-[13px]">
          {status.allowed.map((h) => (
            <li key={h.host}>
              {h.host} <span className="font-sans text-ink-faint">({h.reason})</span>
            </li>
          ))}
        </ul>
      )}
      <h3 className="mt-4 font-medium">Blocked this session</h3>
      {status.blocked.length === 0 ? (
        <p className="text-ink-faint">Nothing was blocked.</p>
      ) : (
        <ul className="mt-1 font-mono text-[13px]">
          {status.blocked.map((b, i) => (
            <li key={`${b.host}-${i}`}>
              {b.host} <span className="font-sans text-ink-faint">({b.reason}, {new Date((b.at ?? 0) * 1000).toLocaleTimeString()})</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
