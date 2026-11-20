import type { ModelEntry_Serialize as ModelEntry, ProviderView_Serialize as ProviderView } from "../ipc/bindings";

export function provider(id: string, over: Partial<ProviderView> = {}): ProviderView {
  return {
    id,
    name: id[0].toUpperCase() + id.slice(1),
    icon: id,
    protocol: "openai_chat",
    baseUrl: `https://api.${id}.example/v1`,
    keyEnv: `${id.toUpperCase()}_API_KEY`,
    signupUrl: null,
    docsUrl: `https://${id}.example/docs`,
    checked: "2026-11-20",
    freeTier: "free_tier",
    freeTierNote: "",
    region: "US",
    tags: [],
    key: { set: { source: "Keychain", masked: "gsk...ABCD" } },
    configured: true,
    disabled: false,
    userPack: false,
    ...over,
  };
}

export function model(providerId: string, id: string, over: Partial<ModelEntry> = {}): ModelEntry {
  return {
    providerId,
    id,
    name: id,
    priceIn: 1,
    priceOut: 2,
    pricingKnown: true,
    free: false,
    context: 128_000,
    tools: true,
    vision: false,
    reasoning: false,
    released: "2026-01-01",
    live: true,
    ...over,
  };
}
