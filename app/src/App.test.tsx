import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks } from "@tauri-apps/api/mocks";
import App from "./App";
import { mockSettings } from "./test/ipc";
import { provider } from "./test/fixtures";

afterEach(() => {
  cleanup();
  clearMocks();
});

describe("App shell", () => {
  it("rail switches pages and marks the current one", async () => {
    mockSettings({});
    render(<App />);
    const rail = await screen.findByRole("navigation", { name: "Main" });
    expect(await screen.findByRole("heading", { name: "New chat" })).toBeTruthy();
    const settings = within(rail).getByRole("button", { name: "Settings" });
    await userEvent.click(settings);
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeTruthy();
    expect(settings.getAttribute("aria-current")).toBe("page");
    await userEvent.click(within(rail).getByRole("button", { name: "Sessions" }));
    expect(await screen.findByRole("heading", { name: "Sessions" })).toBeTruthy();
    await userEvent.click(within(rail).getByRole("button", { name: "Providers" }));
    expect(await screen.findByRole("heading", { name: "Providers" })).toBeTruthy();
  });

  it("shows the first-launch update question", async () => {
    mockSettings({ updateCheck: "notAsked" });
    render(<App />);
    expect(await screen.findByRole("dialog")).toBeTruthy();
  });

  it("Ctrl+K opens the command palette and Enter runs a command", async () => {
    mockSettings({});
    render(<App />);
    await screen.findByRole("heading", { name: "New chat" });
    await userEvent.keyboard("{Control>}k{/Control}");
    const input = await screen.findByRole("combobox", { name: "Type a command" });
    await userEvent.type(input, "providers{Enter}");
    expect(await screen.findByRole("heading", { name: "Providers" })).toBeTruthy();
    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
  });

  it("sidebar lists projects with nested sessions and searches", async () => {
    mockSettings({}, (cmd) => {
      if (cmd === "projects_list") return [{ id: "p1", path: "D:/code/thing", name: "thing" }];
      if (cmd === "sessions_list")
        return [
          { id: "s1", projectId: "p1", title: "Fix the router", updatedAt: 2, model: null },
          { id: "s2", projectId: null, title: "Loose chat", updatedAt: 1, model: null },
        ];
      if (cmd === "sessions_search") return [{ sessionId: "s2", title: "Loose chat", snippet: "the [borrow] checker" }];
      return undefined;
    });
    render(<App />);
    const project = await screen.findByRole("region", { name: "Project thing" });
    expect(within(project).getByRole("button", { name: "Fix the router" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "Chats without a project" })).toBeTruthy();
    await userEvent.type(screen.getByRole("searchbox", { name: "Search chats" }), "borrow");
    expect(await screen.findByText("the [borrow] checker")).toBeTruthy();
  });

  it("provider rail context menu disables a provider", async () => {
    const saved = mockSettings({}, (cmd) => {
      if (cmd === "providers_list") return { providers: [provider("groq")], warnings: [], ageLocked: false };
      return undefined;
    });
    render(<App />);
    await userEvent.click(within(await screen.findByRole("navigation", { name: "Main" })).getByRole("button", { name: "Providers" }));
    const icon = await screen.findByRole("button", { name: "Groq (ready)" });
    icon.focus();
    await userEvent.keyboard("{Shift>}{F10}{/Shift}");
    await userEvent.click(await screen.findByRole("menuitem", { name: "Disable" }));
    expect(saved.at(-1)?.disabledProviders).toEqual(["groq"]);
  });

  it("Alt+Down moves a provider in the rail and saves the order", async () => {
    const saved = mockSettings({}, (cmd) => {
      if (cmd === "providers_list") return { providers: [provider("alpha"), provider("beta")], warnings: [], ageLocked: false };
      return undefined;
    });
    render(<App />);
    await userEvent.click(within(await screen.findByRole("navigation", { name: "Main" })).getByRole("button", { name: "Providers" }));
    const alpha = await screen.findByRole("button", { name: "Alpha (ready)" });
    alpha.focus();
    await userEvent.keyboard("{Alt>}{ArrowDown}{/Alt}");
    expect(saved.at(-1)?.providerOrder).toEqual(["beta", "alpha"]);
  });
});
