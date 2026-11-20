import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { KeychainTest } from "./KeychainTest";

afterEach(() => {
  cleanup();
  clearMocks();
});

describe("KeychainTest", () => {
  it("runs the self test and shows success", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "keychain_self_test") {
        calls++;
        return { ok: true, message: "Keychain works: wrote, read and deleted a test value." };
      }
    });
    render(<KeychainTest />);
    await userEvent.click(screen.getByRole("button", { name: "Test keychain" }));
    expect(await screen.findByText(/Keychain works/)).toBeTruthy();
    expect(calls).toBe(1);
  });

  it("shows the failure reason when no keychain is available", async () => {
    mockIPC(() => ({ ok: false, message: "no OS keychain available: dbus not running" }));
    render(<KeychainTest />);
    await userEvent.click(screen.getByRole("button", { name: "Test keychain" }));
    const status = await screen.findByRole("status");
    expect(status.textContent).toContain("no OS keychain available");
  });
});
