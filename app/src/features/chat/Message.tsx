import type { ModelRef, StoredMessage_Serialize as StoredMessage, Usage_Serialize as Usage } from "../../ipc/bindings";
import { formatCost, formatTokens } from "../../lib/format";
import { Markdown } from "./Markdown";

export function TurnFooter({ model, usage, cost }: { model: ModelRef | null; usage: Usage | null; cost?: number | null }) {
  if (!model && !usage) return null;
  return (
    <p className="mt-2 text-xs text-ink-faint">
      {model && <span>{model.modelId}</span>}
      {usage && (
        <>
          {" · "}
          {formatTokens(usage.input)} in / {formatTokens(usage.output)} out
          {usage.cached > 0 && ` (${formatTokens(usage.cached)} cached)`}
        </>
      )}
      {usage && ` · ${formatCost(cost ?? usage.cost)}`}
    </p>
  );
}

function Reasoning({ text, live }: { text: string; live?: boolean }) {
  if (!text) return null;
  return (
    <details className="mb-2 rounded-lg border border-line bg-surface-1 px-3 py-1.5 text-ink-muted" open={live}>
      <summary className="cursor-pointer text-xs select-none">Reasoning</summary>
      <div className="mt-1 text-[13px] whitespace-pre-wrap">{text}</div>
    </details>
  );
}

export function Message({ message: m }: { message: StoredMessage }) {
  if (m.role === "user") {
    return (
      <article aria-label="You" className="ml-auto max-w-[85%] rounded-2xl bg-surface-3 px-4 py-2.5 whitespace-pre-wrap">
        {m.content}
      </article>
    );
  }
  return (
    <article aria-label="Assistant" className="max-w-full">
      <Reasoning text={m.reasoning ?? ""} />
      {m.content && <Markdown text={m.content} />}
      {m.error && (
        <p role="alert" className="mt-2 rounded-md border border-danger/40 px-3 py-2 text-danger">
          {m.error}
        </p>
      )}
      <TurnFooter model={m.model} usage={m.usage} cost={m.cost} />
    </article>
  );
}

export function StreamingMessage({ text, reasoning, model, usage }: { text: string; reasoning: string; model: ModelRef; usage: Usage | null }) {
  return (
    <article aria-label="Assistant (writing)" aria-busy="true" className="max-w-full">
      <Reasoning text={reasoning} live />
      {text && <Markdown text={text} />}
      <TurnFooter model={model} usage={usage} />
    </article>
  );
}
