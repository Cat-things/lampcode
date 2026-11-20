import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks } from "@tauri-apps/api/mocks";
import App from "./App";
import { mockSettings } from "./test/ipc";

afterEach(() => {
  cleanup();
  clearMocks();
});

describe("App shell", () => {
  it("rail switches between Home and Settings and marks the current page", async () => {
    mockSettings({ theme: "dark", updateCheck: "disabled" });
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Home" })).toBeTruthy();
    const settingsLink = screen.getByRole("button", { name: "Settings" });
    await userEvent.click(settingsLink);
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeTruthy();
    expect(settingsLink.getAttribute("aria-current")).toBe("page");
  });

  it("shows the first-launch update question", async () => {
    mockSettings({ theme: "dark", updateCheck: "notAsked" });
    render(<App />);
    expect(await screen.findByRole("dialog")).toBeTruthy();
  });
});
