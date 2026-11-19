import { appConfig } from "../app.config";

export default function App() {
  return (
    <main className="flex h-full items-center justify-center bg-night text-ink">
      <h1 className="text-2xl font-semibold tracking-tight">{appConfig.name}</h1>
    </main>
  );
}
