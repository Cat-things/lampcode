import { Channel } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  commands,
  type ModelRef,
  type SessionEvent_Deserialize,
  type SessionEvent_Serialize as SessionEvent,
  type StoredMessage_Serialize as StoredMessage,
  type Usage_Serialize as Usage,
} from "../../ipc/bindings";
import { errorText } from "../../lib/format";
import { modelKey, recordSpeed } from "../models/sortModels";

export type Streaming = {
  model: ModelRef;
  text: string;
  reasoning: string;
  usage: Usage | null;
};

export function useChat(sessionId: string | null) {
  const [messages, setMessages] = useState<StoredMessage[]>([]);
  const [streaming, setStreaming] = useState<Streaming | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const activeSession = useRef<string | null>(sessionId);
  /** Set while send() creates a session, so loading it doesn't wipe the turn. */
  const creating = useRef<string | null>(null);

  useEffect(() => {
    activeSession.current = sessionId;
    if (creating.current && creating.current === sessionId) return;
    setStreaming(null);
    setError(null);
    if (!sessionId) {
      setMessages([]);
      return;
    }
    let alive = true;
    commands.sessionMessages(sessionId).then(
      (m) => alive && setMessages(m),
      (e) => alive && setError(errorText(e)),
    );
    return () => {
      alive = false;
    };
  }, [sessionId]);

  const send = useCallback(
    async (text: string, model: ModelRef, ensureSession: () => Promise<string>) => {
      setError(null);
      setBusy(true);
      let sid = activeSession.current;
      try {
        if (!sid) {
          sid = await ensureSession();
          creating.current = sid;
          activeSession.current = sid;
        }
        const target = sid;
        let firstDelta = 0;
        let outputTokens = 0;
        const channel = new Channel<SessionEvent>();
        channel.onmessage = (ev) => {
          if (activeSession.current !== target) return;
          switch (ev.type) {
            case "userSaved":
              setMessages((m) => [...m, ev.message]);
              break;
            case "started":
              setStreaming({ model: ev.model, text: "", reasoning: "", usage: null });
              break;
            case "textDelta":
              if (!firstDelta) firstDelta = performance.now();
              setStreaming((s) => (s ? { ...s, text: s.text + ev.text } : s));
              break;
            case "reasoningDelta":
              if (!firstDelta) firstDelta = performance.now();
              setStreaming((s) => (s ? { ...s, reasoning: s.reasoning + ev.text } : s));
              break;
            case "usage":
              outputTokens = ev.usage.output;
              setStreaming((s) => (s ? { ...s, usage: ev.usage } : s));
              break;
            case "finished": {
              setStreaming(null);
              setMessages((m) => [...m, ev.message]);
              const secs = firstDelta ? (performance.now() - firstDelta) / 1000 : 0;
              if (outputTokens > 20 && secs > 0.2) recordSpeed(modelKey({ providerId: model.providerId, id: model.modelId }), outputTokens / secs);
              break;
            }
            case "error":
              setStreaming(null);
              if (ev.saved) {
                const saved = ev.saved;
                setMessages((m) => [...m, saved]);
              }
              setError(ev.message);
              break;
            default:
              break;
          }
        };
        // The backend sends the serialized shape; the generated arg type is the deserialize one.
        await commands.chatSend(target, model, text, channel as unknown as Channel<SessionEvent_Deserialize>);
      } catch (e) {
        setStreaming(null);
        setError(errorText(e));
      } finally {
        creating.current = null;
        setBusy(false);
      }
      return sid;
    },
    [],
  );

  const cancel = useCallback(() => {
    if (activeSession.current) void commands.chatCancel(activeSession.current);
  }, []);

  return { messages, streaming, busy, error, send, cancel };
}
