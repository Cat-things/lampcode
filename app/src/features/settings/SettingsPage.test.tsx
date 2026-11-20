import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks } from "@tauri-apps/api/mocks";
import { SettingsProvider } from "../../settings/SettingsProvider";
import { SettingsPage } from "./SettingsPage";
import { mockSettings } from "../../test/ipc";

afterEach(() => {
  cleanup();
  clearMocks();
  delete document.documentElement.dataset.theme;
});

const ui = () => render(<SettingsProvider><SettingsPage /></SettingsProvider>);

describe("SettingsPage", () => {
  it("switching theme saves it and applies it to the document", async () => {
    const saved = mockSettings({ theme: "dark", updateCheck: "disabled" });
    ui();
    await userEvent.click(await screen.findByRole("radio", { name: "Light" }));
    expect(saved.at(-1)?.theme).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("update checks can be turned on later", async () => {
    const saved = mockSettings({ theme: "dark", updateCheck: "disabled" });
    ui();
    const toggle = await screen.findByRole("switch", { name: /check for updates/i });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    await userEvent.click(toggle);
    expect(saved.at(-1)?.updateCheck).toBe("enabled");
  });

  it("has the keychain test", async () => {
    mockSettings({ theme: "dark", updateCheck: "disabled" });
    ui();
    expect(await screen.findByRole("button", { name: "Test keychain" })).toBeTruthy();
  });
});
