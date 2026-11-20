// Dev-only UI preview with a fake backend, for visual checks in a normal
// browser: `pnpm dev`, then open http://localhost:1420/preview/
// Never part of the app build (vite only builds index.html).
import { mockIPC } from "@tauri-apps/api/mocks";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "../src/App";
import "../src/styles.css";
import { DEFAULT_SETTINGS } from "../src/test/ipc";
import { model, provider } from "../src/test/fixtures";

const providers = [
  provider("groq", { name: "Groq" }),
  provider("openrouter", { name: "OpenRouter", freeTier: "free_models", tags: ["free", "router"] }),
  provider("ollama", { name: "Ollama", key: "notNeeded", freeTier: "local", protocol: "ollama" }),
  provider("anthropic", { name: "Anthropic", key: "missing", configured: false, freeTier: "none" }),
  provider("openai", { name: "OpenAI", key: "missing", configured: false, freeTier: "none" }),
];
const models: Record<string, ReturnType<typeof model>[]> = {
  groq: [
    model("groq", "llama-3.3-70b-versatile", { name: "Llama 3.3 70B", free: true, priceIn: 0, priceOut: 0, context: 131072 }),
    model("groq", "qwen3-32b", { name: "Qwen3 32B", free: true, priceIn: 0, priceOut: 0, context: 131072, reasoning: true }),
  ],
  openrouter: [
    model("openrouter", "deepseek/deepseek-v4-flash:free", { name: "DeepSeek V4 Flash (free)", free: true, priceIn: 0, priceOut: 0, context: 163840 }),
    model("openrouter", "anthropic/claude-sonnet-5-5", { name: "Claude Sonnet 5.5", priceIn: 3, priceOut: 15, context: 1_000_000, vision: true }),
    model("openrouter", "some/new-model", { name: "some/new-model", pricingKnown: false, priceIn: null, priceOut: null, context: null, tools: null }),
  ],
  ollama: [model("ollama", "qwen3:8b", { name: "qwen3:8b", free: true, priceIn: 0, priceOut: 0, context: 40960 })],
};
const now = Date.now();
const sessions = [
  { id: "s1", projectId: "p1", title: "Why is the router double-rendering", updatedAt: now - 60_000, model: { providerId: "groq", modelId: "llama-3.3-70b-versatile" } },
  { id: "s2", projectId: "p1", title: "Write tests for the SSE parser", updatedAt: now - 3_600_000, model: null },
  { id: "s3", projectId: null, title: "Rust lifetimes refresher", updatedAt: now - 86_400_000, model: null },
];
const messages = [
  { id: "m1", role: "user", content: "Why does my React router render twice in dev?", reasoning: null, toolCalls: [], toolCallId: null, model: null, usage: null, cost: null, error: null, createdAt: now },
  {
    id: "m2",
    role: "assistant",
    content:
      "That's **StrictMode**. In development React mounts, unmounts and mounts again to surface effects that don't clean up.\n\n```tsx\nuseEffect(() => {\n  const id = setInterval(tick, 1000);\n  return () => clearInterval(id); // cleanup\n}, []);\n```\n\n- It only happens in dev\n- Production renders once",
    reasoning: "User sees double renders in dev. Most likely StrictMode.",
    toolCalls: [],
    toolCallId: null,
    model: { providerId: "groq", modelId: "llama-3.3-70b-versatile" },
    usage: { input: 1840, output: 212, cached: 0, cost: null },
    cost: 0,
    error: null,
    createdAt: now,
  },
];
let settings = { ...DEFAULT_SETTINGS, lastModel: { providerId: "groq", modelId: "llama-3.3-70b-versatile" } };

mockIPC((cmd, args) => {
  const a = (args ?? {}) as Record<string, unknown>;
  switch (cmd) {
    case "settings_get":
      return settings;
    case "settings_set":
      settings = a.settings as typeof settings;
      return null;
    case "providers_list":
      return { providers, warnings: [], ageLocked: false };
    case "models_list":
    case "models_refresh":
      return models[a.providerId as string] ?? [];
    case "ratelimits":
      return { groq: { requestsLimit: 1000, requestsRemaining: 640, tokensLimit: null, tokensRemaining: null, resetSecs: 30 } };
    case "projects_list":
      return [{ id: "p1", path: "D:/code/shop-frontend", name: "shop-frontend" }];
    case "sessions_list":
      return sessions;
    case "session_messages":
      return a.sessionId === "s1" ? messages : [];
    case "sessions_search":
      return [];
    case "catalog_info":
      return "2026-11-20";
    case "network_status":
      return { allowed: [{ host: "api.groq.com", reason: "configured provider" }], blocked: [] };
    case "app_info":
      return { name: "LampCode", version: "0.1.0", dataDir: "C:/Users/you/AppData/Roaming/dev.lampstudios.lampcode" };
    default:
      return null;
  }
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
