import { useState } from "react";
import { Home } from "./features/home/Home";
import { SettingsPage } from "./features/settings/SettingsPage";
import { UpdatePrompt } from "./features/onboarding/UpdatePrompt";
import { SettingsProvider } from "./settings/SettingsProvider";

type Page = "home" | "settings";

const PAGES: { id: Page; label: string }[] = [
  { id: "home", label: "Home" },
  { id: "settings", label: "Settings" },
];

export default function App() {
  const [page, setPage] = useState<Page>("home");

  return (
    <SettingsProvider>
      <div className="flex h-full bg-night text-ink">
        <nav aria-label="Main" className="flex w-40 shrink-0 flex-col gap-1 border-r border-line bg-surface-1 p-2">
          {PAGES.map((p) => (
            <button
              key={p.id}
              type="button"
              aria-current={page === p.id ? "page" : undefined}
              onClick={() => setPage(p.id)}
              className="h-8 rounded-md px-3 text-left text-ink-muted hover:bg-surface-2 hover:text-ink aria-[current=page]:bg-surface-3 aria-[current=page]:text-ink"
            >
              {p.label}
            </button>
          ))}
        </nav>
        <main className="min-w-0 flex-1 overflow-auto">{page === "home" ? <Home /> : <SettingsPage />}</main>
      </div>
      <UpdatePrompt />
    </SettingsProvider>
  );
}
