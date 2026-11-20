import { useEffect, useState } from "react";

const VERBS = ["Polishing", "Untangling", "Reading up on", "Tinkering with", "Squinting at", "Sweeping"];

function prefersReducedMotion() {
  return typeof window !== "undefined" && !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
}

/** "Polishing <project>..." while the model thinks. Static under reduced motion. */
export function ThinkingLine({ subject }: { subject: string }) {
  const [i, setI] = useState(0);
  const reduced = prefersReducedMotion();
  useEffect(() => {
    if (reduced) return;
    const t = window.setInterval(() => setI((n) => (n + 1) % VERBS.length), 2400);
    return () => window.clearInterval(t);
  }, [reduced]);
  const verb = reduced ? VERBS[0] : VERBS[i];
  return (
    <p role="status" aria-live="polite" className="thinking-line text-ink-muted">
      <span className={reduced ? undefined : "thinking-shimmer"}>
        {verb} {subject}
      </span>
      <span aria-hidden="true" className={reduced ? undefined : "thinking-dots"}>
        ...
      </span>
    </p>
  );
}
