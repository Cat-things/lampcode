import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks } from "@tauri-apps/api/mocks";
import type { SessionEvent_Serialize as SessionEvent, StoredMessage_Serialize as StoredMessage } from "../../ipc/bindings";
import { SettingsProvider } from "../../settings/SettingsProvider";
import { AppDataProvider } from "../../state/AppData";
import { mockSettings } from "../../test/ipc";
import { provider } from "../../test/fixtures";
import { Chat } from "./Chat";

afterEach(() => {
  cleanup();
  clearMocks();
});

const msg = (role: "user" | "assistant", content: string, over: Partial<StoredMessage> = {}): StoredMessage => ({
  id: `${role}-${content}`,
  role,
  content,
  reasoning: null,
  toolCalls: [],
  toolCallId: null,
  model: null,
  usage: null,
  cost: null,
  error: null,
  createdAt: 1,
  ...over,
});

type Emitter = { onmessage: (e: SessionEvent) => void };

function setup(script: (emit: (e: SessionEvent) => void) => void) {
  const sent: { text: string; sessionId: string }[] = [];
  const created: unknown[] = [];
  mockSettings({ lastModel: { providerId: "groq", modelId: "llama" } }, (cmd, args) => {
    if (cmd === "providers_list") return { providers: [provider("groq")], warnings: [], ageLocked: false };
    if (cmd === "session_create") {
      created.push(args);
      return { id: "s1", projectId: null, title: "", updatedAt: 1, model: args.model };
    }
    if (cmd === "chat_send") {
      sent.push({ text: args.text as string, sessionId: args.sessionId as string });
      const ch = args.onEvent as Emitter;
      script((e) => ch.onmessage(e));
      return null;
    }
    return undefined;
  });
  render(
    <SettingsProvider>
      <AppDataProvider lastModel={{ providerId: "groq", modelId: "llama" }}>
        <Chat sessionId={null} projectId={null} />
      </AppDataProvider>
    </SettingsProvider>,
  );
  return { sent, created };
}

describe("Chat", () => {
  it("creates a session on first send, streams the reply and shows tokens and cost", async () => {
    const { sent, created } = setup((emit) => {
      emit({ type: "userSaved", message: msg("user", "hi there") });
      emit({ type: "started", messageId: "a1", model: { providerId: "groq", modelId: "llama" } });
      emit({ type: "textDelta", text: "Hello " });
      emit({ type: "textDelta", text: "**world**" });
      emit({
        type: "finished",
        stopReason: "stop",
        message: msg("assistant", "Hello **world**", {
          model: { providerId: "groq", modelId: "llama" },
          usage: { input: 1200, output: 30, cached: 0, cost: null },
          cost: 0,
        }),
      });
    });
    const box = await screen.findByRole("textbox", { name: "Message" });
    await userEvent.type(box, "hi there{Enter}");
    expect(await screen.findByText("world")).toBeTruthy();
    expect(screen.getByText("world").tagName).toBe("STRONG");
    expect(screen.getByText(/1,200 in \/ 30 out/)).toBeTruthy();
    expect(screen.getByText(/free/)).toBeTruthy();
    expect(sent).toEqual([{ text: "hi there", sessionId: "s1" }]);
    expect(created).toHaveLength(1);
    expect((box as HTMLTextAreaElement).value).toBe("");
  });

  it("shows provider errors and keeps partial text", async () => {
    setup((emit) => {
      emit({ type: "userSaved", message: msg("user", "go") });
      emit({ type: "error", message: "rate limited, retry in 12s", saved: msg("assistant", "half an answ", { error: "rate limited, retry in 12s" }) });
    });
    await userEvent.type(await screen.findByRole("textbox", { name: "Message" }), "go{Enter}");
    expect(await screen.findByText("half an answ")).toBeTruthy();
    expect(screen.getAllByRole("alert").map((a) => a.textContent)).toEqual(["rate limited, retry in 12s"]);
  });

  it("model links do not navigate and images do not load", async () => {
    setup((emit) => {
      emit({ type: "userSaved", message: msg("user", "x") });
      emit({
        type: "finished",
        stopReason: "stop",
        message: msg("assistant", "see [docs](https://evil.example/x) ![pic](https://evil.example/p.png) <b>raw</b>"),
      });
    });
    await userEvent.type(await screen.findByRole("textbox", { name: "Message" }), "x{Enter}");
    await screen.findByText("docs");
    expect(document.querySelector("a")).toBeNull();
    expect(document.querySelector("img")).toBeNull();
    expect(document.querySelector("article b")).toBeNull();
    expect(screen.getByText("(https://evil.example/x)")).toBeTruthy();
  });
});
