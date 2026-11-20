import { describe, expect, it } from "vitest";
import { monogram } from "./ProviderIcon";

describe("monogram", () => {
  it("keeps similar provider names apart", () => {
    expect(monogram("OpenAI")).toBe("OA");
    expect(monogram("OpenRouter")).toBe("OR");
    expect(monogram("Ollama")).toBe("OL");
    expect(monogram("NVIDIA NIM")).toBe("NN");
    expect(monogram("Google Gemini")).toBe("GG");
    expect(monogram("Groq")).toBe("GR");
  });
});
