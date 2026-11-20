import { useEffect, useRef, useState } from "react";
import { commands, type ModelRef } from "../../ipc/bindings";
import { useAppData } from "../../state/AppData";
import { Composer } from "./Composer";
import { Message, StreamingMessage } from "./Message";
import { ThinkingLine } from "./ThinkingLine";
import { useChat } from "./useChat";

export function Chat({ sessionId, projectId }: { sessionId: string | null; projectId: string | null }) {
  const { sessions, projects, providers, defaultModel, setView, reloadSessions } = useAppData();
  const session = sessions.find((s) => s.id === sessionId);
  const project = projects.find((p) => p.id === (session?.projectId ?? projectId));
  const { messages, streaming, busy, error, send, cancel } = useChat(sessionId);
  const [model, setModel] = useState<ModelRef | null>(session?.model ?? defaultModel);
  const bottom = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (session?.model) setModel(session.model);
  }, [session?.model]);

  useEffect(() => {
    bottom.current?.scrollIntoView?.({ block: "end" });
  }, [messages.length, streaming?.text]);

  const modelProvider = providers.find((p) => p.id === model?.providerId);
  const modelUnavailable = model && modelProvider && (!modelProvider.configured || modelProvider.disabled);

  async function onSend(text: string) {
    if (!model) return;
    await send(text, model, async () => {
      const s = await commands.sessionCreate(projectId, model);
      setView({ kind: "chat", sessionId: s.id, projectId });
      return s.id;
    });
    await reloadSessions();
  }

  const empty = messages.length === 0 && !streaming && !busy;
  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex h-11 shrink-0 items-center border-b border-line px-5">
        <h1 className="truncate font-medium">{session?.title || (project ? `New chat in ${project.name}` : "New chat")}</h1>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto flex max-w-3xl flex-col gap-5 px-5 py-6">
          {empty && (
            <div className="mt-[18vh] text-center">
              <p className="text-2xl font-semibold">What are we building?</p>
              <p className="mt-2 text-ink-muted">
                {model ? `Chatting with ${model.modelId}.` : "Pick a model below to start. Free ones are marked FREE."}
              </p>
            </div>
          )}
          {messages.map((m) => (
            <Message key={m.id} message={m} />
          ))}
          {busy && !streaming?.text && !streaming?.reasoning && <ThinkingLine subject={project?.name ?? "your question"} />}
          {streaming && (streaming.text || streaming.reasoning) && (
            <StreamingMessage text={streaming.text} reasoning={streaming.reasoning} model={streaming.model} usage={streaming.usage} />
          )}
          {error && !messages.some((m) => m.error === error) && (
            <p role="alert" className="rounded-md border border-danger/40 px-3 py-2 text-danger">
              {error}
            </p>
          )}
          <div ref={bottom} />
        </div>
      </div>
      <div className="mx-auto w-full max-w-3xl shrink-0 px-5 pb-5">
        {modelUnavailable && (
          <p className="mb-2 text-ink-muted">
            {modelProvider?.name} isn't set up right now. Pick another model or fix it on the Providers page.
          </p>
        )}
        <Composer model={model} onModel={setModel} busy={busy} onSend={onSend} onCancel={cancel} />
      </div>
    </div>
  );
}
