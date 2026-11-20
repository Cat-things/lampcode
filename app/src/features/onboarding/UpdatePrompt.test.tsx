import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks } from "@tauri-apps/api/mocks";
import { SettingsProvider } from "../../settings/SettingsProvider";
import { UpdatePrompt } from "./UpdatePrompt";
import { mockSettings } from "../../test/ipc";

afterEach(() => {
  cleanup();
  clearMocks();
});

const ui = () => render(<SettingsProvider><UpdatePrompt /></SettingsProvider>);

describe("UpdatePrompt", () => {
  it("asks once on first launch and saves 'no'", async () => {
    const saved = mockSettings({ theme: "dark", updateCheck: "notAsked" });
    ui();
    const dialog = await screen.findByRole("dialog", { name: /check for updates/i });
    expect(dialog.textContent).toContain("GitHub Releases");
    await userEvent.click(screen.getByRole("button", { name: "No, don't check" }));
    expect(saved.at(-1)?.updateCheck).toBe("disabled");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("saves 'yes'", async () => {
    const saved = mockSettings({ theme: "dark", updateCheck: "notAsked" });
    ui();
    await userEvent.click(await screen.findByRole("button", { name: "Yes, check for updates" }));
    expect(saved.at(-1)?.updateCheck).toBe("enabled");
  });

  it("stays hidden once answered", async () => {
    mockSettings({ theme: "dark", updateCheck: "disabled" });
    render(<SettingsProvider><UpdatePrompt /><p>loaded</p></SettingsProvider>);
    await screen.findByText("loaded");
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
