import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { Home } from "./Home";

afterEach(() => {
  cleanup();
  clearMocks();
});

describe("Home", () => {
  it("shows app info from the backend", async () => {
    mockIPC((cmd) => {
      if (cmd === "app_info") {
        return { name: "LampCode", version: "9.9.9", dataDir: "C:/data/LampCode" };
      }
    });
    render(<Home />);
    expect(await screen.findByText("9.9.9")).toBeTruthy();
    expect(screen.getByText("C:/data/LampCode")).toBeTruthy();
  });

  it("shows a readable error when the backend call fails", async () => {
    mockIPC(() => {
      throw new Error("no data dir");
    });
    render(<Home />);
    expect(await screen.findByRole("alert")).toBeTruthy();
  });
});
