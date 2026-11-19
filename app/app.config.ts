// Single rename point (together with src-tauri/tauri.conf.json, kept in sync by app.config.test.ts).
export const appConfig = {
  name: "LampCode",
  identifier: "dev.lampstudios.lampcode",
  repoUrl: "https://github.com/lampstudios/lampcode",
  releasesUrl: "https://github.com/lampstudios/lampcode/releases",
} as const;

export type AppConfig = typeof appConfig;
