import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks } from "@tauri-apps/api/mocks";
import { SettingsProvider } from "../../settings/SettingsProvider";
import { AppDataProvider } from "../../state/AppData";
import { mockSettings } from "../../test/ipc";
import { model, provider } from "../../test/fixtures";
import { ModelPicker } from "./ModelPicker";

afterEach(() => {
  cleanup();
  clearMocks();
});

function setup(providers = [provider("groq"), provider("openai", { configured: false, key: "missing" })]) {
  const models = {
    groq: [model("groq", "llama-free", { free: true, priceIn: 0, priceOut: 0 }), model("groq", "big-paid", { priceIn: 5, priceOut: 15 })],
    openai: [model("openai", "gpt-x")],
  } as Record<string, ReturnType<typeof model>[]>;
  const listCalls: string[] = [];
  const saved = mockSettings({}, (cmd, args) => {
    if (cmd === "providers_list") return { providers, warnings: [], ageLocked: false };
    if (cmd === "models_list") {
      listCalls.push(args.providerId as string);
      return models[args.providerId as string] ?? [];
    }
    if (cmd === "ratelimits") return { groq: { requestsLimit: 100, requestsRemaining: 50, tokensLimit: null, tokensRemaining: null, resetSecs: null } };
    return undefined;
  });
  const onChange = vi.fn();
  render(
    <SettingsProvider>
      <AppDataProvider lastModel={null}>
        <ModelPicker value={null} onChange={onChange} />
      </AppDataProvider>
    </SettingsProvider>,
  );
  return { onChange, saved, listCalls };
}

describe("ModelPicker", () => {
  it("lists models from configured providers only, with FREE badge and headroom", async () => {
    const { listCalls } = setup();
    await userEvent.click(await screen.findByRole("button", { name: "Choose a model" }));
    const list = await screen.findByRole("listbox", { name: "Models" });
    await within(list).findByText("llama-free");
    expect(within(list).queryByText("gpt-x")).toBeNull();
    expect(listCalls).not.toContain("openai");
    expect(within(list).getByText("FREE")).toBeTruthy();
    expect(within(list).getAllByRole("img", { name: "Rate limit headroom 50%" }).length).toBe(2);
    // Cheapest first by default.
    const names = within(list).getAllByRole("option").map((o) => o.textContent ?? "");
    expect(names[0]).toContain("llama-free");
  });

  it("free only hides paid models", async () => {
    setup();
    await userEvent.click(await screen.findByRole("button", { name: "Choose a model" }));
    await screen.findByText("big-paid");
    await userEvent.click(screen.getByRole("switch", { name: "Free only" }));
    expect(screen.queryByText("big-paid")).toBeNull();
    expect(screen.getByText("llama-free")).toBeTruthy();
  });

  it("starring a model saves it to favorites", async () => {
    const { saved } = setup();
    await userEvent.click(await screen.findByRole("button", { name: "Choose a model" }));
    await userEvent.click(await screen.findByRole("button", { name: "Star big-paid" }));
    expect(saved.at(-1)?.favoriteModels).toEqual(["groq/big-paid"]);
  });

  it("picking a model calls onChange and closes", async () => {
    const { onChange } = setup();
    await userEvent.click(await screen.findByRole("button", { name: "Choose a model" }));
    await userEvent.click(await screen.findByText("big-paid"));
    expect(onChange).toHaveBeenCalledWith({ providerId: "groq", modelId: "big-paid" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("explains what to do when nothing is configured", async () => {
    setup([provider("openai", { configured: false, key: "missing" })]);
    await userEvent.click(await screen.findByRole("button", { name: "Choose a model" }));
    expect(await screen.findByText(/No provider is set up yet/)).toBeTruthy();
  });
});
