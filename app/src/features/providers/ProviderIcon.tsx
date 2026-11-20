/** Monogram tile for a provider. No brand logos are bundled. */
export function ProviderIcon({ id, name, size = 28, dim = false }: { id: string; name: string; size?: number; dim?: boolean }) {
  let h = 0;
  for (const c of id) h = (h * 31 + c.charCodeAt(0)) % 360;
  const letters = monogram(name);
  return (
    <span
      aria-hidden="true"
      className="inline-flex shrink-0 items-center justify-center rounded-md font-semibold"
      style={{
        width: size,
        height: size,
        fontSize: size * 0.4,
        background: `hsl(${h} 45% ${dim ? 22 : 30}%)`,
        color: `hsl(${h} 80% ${dim ? 60 : 88}%)`,
        filter: dim ? "grayscale(0.9)" : undefined,
        opacity: dim ? 0.55 : 1,
      }}
    >
      {letters}
    </span>
  );
}

/** Two letters: initials of the first two words, else first + next capital, else first two letters. */
export function monogram(name: string): string {
  const words = name.split(/[\s\-_.]+/).filter(Boolean);
  if (words.length >= 2) return (words[0][0] + words[1][0]).toUpperCase();
  const w = words[0] ?? "?";
  const cap = w.slice(1).match(/[A-Z0-9]/);
  return (w[0] + (cap ? cap[0] : (w[1] ?? ""))).toUpperCase();
}
