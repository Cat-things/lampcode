// Single rename point (together with src-tauri/tauri.conf.json, kept in sync by app.config.test.ts).
export const appConfig = {
  name: "LampCode",
  identifier: "dev.lampstudios.lampcode",
  repoUrl: "https://github.com/Cat-things/lampcode",
  releasesUrl: "https://github.com/Cat-things/lampcode/releases",
} as const;

export type AppConfig = typeof appConfig;
