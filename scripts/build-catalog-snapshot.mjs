// Builds app/src-tauri/resources/models.snapshot.json from a models.dev
// api.json export. Manual step: download https://models.dev/api.json yourself,
// then run `node scripts/build-catalog-snapshot.mjs <path-to-api.json>`.
// models.dev data is MIT licensed (see THIRD_PARTY_NOTICES.md).
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const src = process.argv[2];
if (!src) {
  console.error("usage: node scripts/build-catalog-snapshot.mjs <api.json>");
  process.exit(1);
}
const raw = JSON.parse(readFileSync(src, "utf8"));
const providers = {};
for (const [pid, p] of Object.entries(raw).sort(([a], [b]) => a.localeCompare(b))) {
  const models = {};
  for (const [mid, m] of Object.entries(p.models ?? {}).sort(([a], [b]) => a.localeCompare(b))) {
    const cost = m.cost ?? null;
    models[mid] = {
      name: m.name ?? mid,
      price_in: cost && typeof cost.input === "number" ? cost.input : null,
      price_out: cost && typeof cost.output === "number" ? cost.output : null,
      context: m.limit?.context ?? null,
      tools: Boolean(m.tool_call),
      vision: (m.modalities?.input ?? []).includes("image"),
      reasoning: Boolean(m.reasoning),
      released: m.release_date ?? null,
    };
  }
  providers[pid] = { name: p.name ?? pid, models };
}
const out = {
  source: "https://models.dev/api.json",
  fetched: new Date().toISOString().slice(0, 10),
  providers,
};
const here = dirname(fileURLToPath(import.meta.url));
const dest = join(here, "..", "app", "src-tauri", "resources", "models.snapshot.json");
writeFileSync(dest, JSON.stringify(out) + "\n");
console.log(`wrote ${dest}: ${Object.keys(providers).length} providers`);
