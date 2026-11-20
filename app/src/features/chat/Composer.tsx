import { useEffect, useRef, useState } from "react";
import type { ModelRef } from "../../ipc/bindings";
import { IconSend, IconStop } from "../../lib/icons";
import { ModelPicker } from "../models/ModelPicker";

export function Composer({
  model,
  onModel,
  busy,
  onSend,
  onCancel,
}: {
  model: ModelRef | null;
  onModel: (m: ModelRef) => void;
  busy: boolean;
  onSend: (text: string) => void;
  onCancel: () => void;
}) {
  const [text, setText] = useState("");
  const area = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 240)}px`;
  }, [text]);

  useEffect(() => {
    area.current?.focus();
  }, []);

  const canSend = !busy && !!model && text.trim().length > 0;
  function submit() {
    if (!canSend) return;
    onSend(text);
    setText("");
  }

  return (
    <form
      aria-label="Message"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
      className="rounded-2xl border border-line-strong bg-surface-1 p-2 focus-within:border-beam/60"
    >
      <label htmlFor="composer" className="sr-only">
        Message
      </label>
      <textarea
        id="composer"
        ref={area}
        rows={1}
        value={text}
        placeholder={model ? "Ask anything. Enter to send, Shift+Enter for a new line." : "Pick a model first."}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
            e.preventDefault();
            submit();
          } else if (e.key === "Escape" && busy) {
            e.preventDefault();
            onCancel();
          }
        }}
        className="block max-h-60 w-full resize-none bg-transparent px-2 py-1.5 outline-none placeholder:text-ink-faint"
      />
      <div className="mt-1 flex items-center justify-between gap-2">
        <ModelPicker value={model} onChange={onModel} />
        {busy ? (
          <button
            type="button"
            onClick={onCancel}
            aria-label="Stop (Esc)"
            title="Stop (Esc)"
            className="flex h-8 w-8 items-center justify-center rounded-lg bg-surface-3 hover:text-danger"
          >
            <IconStop />
          </button>
        ) : (
          <button
            type="submit"
            disabled={!canSend}
            aria-label="Send"
            className="flex h-8 w-8 items-center justify-center rounded-lg bg-beam text-beam-ink disabled:opacity-40"
          >
            <IconSend />
          </button>
        )}
      </div>
    </form>
  );
}
