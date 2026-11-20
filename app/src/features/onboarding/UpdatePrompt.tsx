import { useEffect, useRef } from "react";
import { useSettings } from "../../settings/SettingsProvider";

/** Asked exactly once. Until answered "yes", the app makes no update requests. */
export function UpdatePrompt() {
  const { settings, update } = useSettings();
  const noRef = useRef<HTMLButtonElement>(null);
  const open = settings.updateCheck === "notAsked";

  useEffect(() => {
    if (open) noRef.current?.focus();
  }, [open]);

  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="update-prompt-title"
        className="w-full max-w-md rounded-xl border border-line-strong bg-surface-3 p-6 shadow-2xl"
      >
        <h2 id="update-prompt-title" className="text-lg font-semibold">
          Check for updates?
        </h2>
        <p className="mt-2 text-ink-muted">
          LampCode can check GitHub Releases for new versions. If you say no, it never contacts GitHub. You can
          change this later in Settings.
        </p>
        <div className="mt-5 flex justify-end gap-2">
          <button
            ref={noRef}
            type="button"
            onClick={() => update({ updateCheck: "disabled" })}
            className="h-9 rounded-md border border-line-strong px-4 hover:bg-surface-2"
          >
            No, don't check
          </button>
          <button
            type="button"
            onClick={() => update({ updateCheck: "enabled" })}
            className="h-9 rounded-md bg-beam px-4 font-medium text-beam-ink hover:opacity-90"
          >
            Yes, check for updates
          </button>
        </div>
      </div>
    </div>
  );
}
