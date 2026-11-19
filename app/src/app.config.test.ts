import { describe, expect, it } from "vitest";
import tauriConf from "../src-tauri/tauri.conf.json";
import { appConfig } from "../app.config";

// The app name lives in exactly two places: app.config.ts and tauri.conf.json.
// This test keeps them in sync so a rename can't half-happen.
describe("app config", () => {
  it("product name matches tauri.conf.json", () => {
    expect(tauriConf.productName).toBe(appConfig.name);
  });

  it("main window title matches the app name", () => {
    expect(tauriConf.app.windows[0].title).toBe(appConfig.name);
  });

  it("identifier matches tauri.conf.json", () => {
    expect(tauriConf.identifier).toBe(appConfig.identifier);
  });
});
