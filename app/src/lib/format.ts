/** USD per 1M tokens, compact. */
export function formatPrice(perMillion: number | null | undefined): string {
  if (perMillion == null) return "?";
  if (perMillion === 0) return "$0";
  if (perMillion < 0.01) return `$${perMillion.toFixed(4)}`;
  if (perMillion < 10) return `$${perMillion.toFixed(2)}`;
  return `$${perMillion.toFixed(0)}`;
}

export function formatContext(tokens: number | null | undefined): string {
  if (!tokens) return "?";
  if (tokens >= 1_000_000) return `${+(tokens / 1_000_000).toFixed(1)}M`;
  if (tokens >= 1000) return `${Math.round(tokens / 1000)}k`;
  return String(tokens);
}

export function formatTokens(n: number): string {
  return n >= 10_000 ? `${(n / 1000).toFixed(1)}k` : n.toLocaleString("en-US");
}

export function formatCost(usd: number | null | undefined): string {
  if (usd == null) return "cost unknown";
  if (usd === 0) return "free";
  if (usd < 0.0001) return "<$0.0001";
  return `$${usd < 0.01 ? usd.toFixed(4) : usd.toFixed(3)}`;
}

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}
