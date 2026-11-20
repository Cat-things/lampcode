import type { ReactNode } from "react";
import type { Theme } from "../../ipc/bindings";
import { useSettings } from "../../settings/SettingsProvider";
import { About } from "./About";
import { KeychainTest } from "./KeychainTest";
import { NetworkList } from "./NetworkList";

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="border-t border-line py-6 first:border-t-0">
      <h2 className="mb-3 text-base font-semibold">{title}</h2>
      {children}
    </section>
  );
}

const THEMES: { value: Theme; label: string }[] = [
  { value: "dark", label: "Dark" },
  { value: "light", label: "Light" },
  { value: "system", label: "Match system" },
];

export function SettingsPage() {
  const { settings, update } = useSettings();
  const updatesOn = settings.updateCheck === "enabled";

  return (
    <div className="mx-auto max-w-2xl p-8">
      <h1 className="text-xl font-semibold tracking-tight">Settings</h1>

      <Section title="Appearance">
        <div role="radiogroup" aria-label="Theme" className="inline-flex rounded-lg border border-line p-1">
          {THEMES.map((t) => (
            <button
              key={t.value}
              type="button"
              role="radio"
              aria-checked={settings.theme === t.value}
              onClick={() => update({ theme: t.value })}
              className="h-8 rounded-md px-3 text-ink-muted aria-checked:bg-surface-3 aria-checked:text-ink"
            >
              {t.label}
            </button>
          ))}
        </div>
      </Section>

      <Section title="Keys">
        <p className="mb-3 text-ink-muted">
          API keys are stored in your system keychain. Environment variables like <code>GROQ_API_KEY</code> override
          saved keys.
        </p>
        <KeychainTest />
      </Section>

      <Section title="Updates">
        <div className="flex items-center justify-between gap-4">
          <div>
            <p id="updates-label">Check for updates on GitHub Releases</p>
            <p className="text-ink-faint">Off means LampCode never contacts GitHub.</p>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={updatesOn}
            aria-labelledby="updates-label"
            onClick={() => update({ updateCheck: updatesOn ? "disabled" : "enabled" })}
            className="relative h-6 w-11 shrink-0 rounded-full border border-line-strong bg-surface-3 transition-colors aria-checked:bg-beam"
          >
            <span
              aria-hidden
              className={`absolute top-0.5 h-4 w-4 rounded-full bg-ink transition-transform ${updatesOn ? "translate-x-5 bg-beam-ink" : "translate-x-0.5"}`}
            />
          </button>
        </div>
      </Section>
      <Section title="Privacy">
        <p className="mb-3 text-ink-muted">
          No account, no telemetry. LampCode only talks to the hosts below, derived from the providers you set up.
        </p>
        <NetworkList />
      </Section>

      <Section title="About">
        <About />
      </Section>
    </div>
  );
}
