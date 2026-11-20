//! Model catalog: a bundled models.dev snapshot merged with live `/models`
//! results. Live results win for their provider.

use super::{ModelInfo, Preset, preset::FreeTier};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::RwLock;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SnapModel {
    pub name: String,
    pub price_in: Option<f64>,
    pub price_out: Option<f64>,
    pub context: Option<u32>,
    #[serde(default)]
    pub tools: bool,
    #[serde(default)]
    pub vision: bool,
    #[serde(default)]
    pub reasoning: bool,
    pub released: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SnapProvider {
    pub name: String,
    pub models: BTreeMap<String, SnapModel>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub source: String,
    pub fetched: String,
    pub providers: HashMap<String, SnapProvider>,
}

impl Snapshot {
    /// Convert a raw models.dev `api.json` (same transform as
    /// `scripts/build-catalog-snapshot.mjs`).
    pub fn from_models_dev(raw: &Value, fetched: &str) -> Result<Self, String> {
        let obj = raw.as_object().ok_or("models.dev data is not an object")?;
        let mut providers = HashMap::new();
        for (pid, p) in obj {
            let mut models = BTreeMap::new();
            if let Some(ms) = p.get("models").and_then(Value::as_object) {
                for (mid, m) in ms {
                    let inputs = m.pointer("/modalities/input").and_then(Value::as_array);
                    models.insert(
                        mid.clone(),
                        SnapModel {
                            name: m.get("name").and_then(Value::as_str).unwrap_or(mid).to_string(),
                            price_in: m.pointer("/cost/input").and_then(Value::as_f64),
                            price_out: m.pointer("/cost/output").and_then(Value::as_f64),
                            context: m.pointer("/limit/context").and_then(Value::as_u64).map(|c| c as u32),
                            tools: m.get("tool_call").and_then(Value::as_bool).unwrap_or(false),
                            vision: inputs.is_some_and(|a| a.iter().any(|x| x == "image")),
                            reasoning: m.get("reasoning").and_then(Value::as_bool).unwrap_or(false),
                            released: m.get("release_date").and_then(Value::as_str).map(String::from),
                        },
                    );
                }
            }
            let name = p.get("name").and_then(Value::as_str).unwrap_or(pid).to_string();
            providers.insert(pid.clone(), SnapProvider { name, models });
        }
        if providers.is_empty() {
            return Err("models.dev data has no providers".into());
        }
        Ok(Snapshot {
            source: "https://models.dev/api.json".into(),
            fetched: fetched.into(),
            providers,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub provider_id: String,
    pub id: String,
    pub name: String,
    /// USD per 1M tokens.
    pub price_in: Option<f64>,
    pub price_out: Option<f64>,
    pub pricing_known: bool,
    pub free: bool,
    pub context: Option<u32>,
    pub tools: Option<bool>,
    pub vision: Option<bool>,
    pub reasoning: Option<bool>,
    /// `YYYY-MM-DD` when known.
    pub released: Option<String>,
    /// The provider's live model list included it.
    pub live: bool,
}

pub struct Catalog {
    snapshot: RwLock<Snapshot>,
    live: RwLock<HashMap<String, Vec<ModelInfo>>>,
}

pub const SNAPSHOT_FILE: &str = "models.snapshot.json";

impl Catalog {
    pub fn new(snapshot: Snapshot) -> Self {
        Self {
            snapshot: RwLock::new(snapshot),
            live: RwLock::new(HashMap::new()),
        }
    }

    /// A refreshed snapshot in the data dir wins over the bundled one.
    pub fn load(bundled: &Path, data_dir: &Path) -> Self {
        let read = |p: &Path| -> Option<Snapshot> { serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok() };
        let snap = read(&data_dir.join(SNAPSHOT_FILE))
            .or_else(|| read(bundled))
            .unwrap_or_default();
        Self::new(snap)
    }

    pub fn snapshot_date(&self) -> String {
        self.snapshot.read().unwrap().fetched.clone()
    }

    pub fn replace_snapshot(&self, s: Snapshot) {
        *self.snapshot.write().unwrap() = s;
    }

    pub fn set_live(&self, provider_id: &str, models: Vec<ModelInfo>) {
        self.live.write().unwrap().insert(provider_id.to_string(), models);
    }

    pub fn has_live(&self, provider_id: &str) -> bool {
        self.live.read().unwrap().contains_key(provider_id)
    }

    pub fn models(&self, preset: &Preset) -> Vec<ModelEntry> {
        let snap = self.snapshot.read().unwrap();
        let empty = BTreeMap::new();
        let snap_models = preset
            .catalog_id
            .as_deref()
            .and_then(|c| snap.providers.get(c))
            .map(|p| &p.models)
            .unwrap_or(&empty);
        let local = preset.free_tier == FreeTier::Local;
        let entry = |id: &str, info: Option<&ModelInfo>, s: Option<&SnapModel>| {
            let price_in = info
                .and_then(|i| i.price_in)
                .or(s.and_then(|s| s.price_in))
                .or(local.then_some(0.0));
            let price_out = info
                .and_then(|i| i.price_out)
                .or(s.and_then(|s| s.price_out))
                .or(local.then_some(0.0));
            let pricing_known = price_in.is_some() && price_out.is_some();
            let zero = pricing_known && price_in == Some(0.0) && price_out == Some(0.0);
            ModelEntry {
                provider_id: preset.id.clone(),
                id: id.to_string(),
                name: info
                    .and_then(|i| i.name.clone())
                    .or(s.map(|s| s.name.clone()))
                    .unwrap_or_else(|| id.to_string()),
                price_in,
                price_out,
                pricing_known,
                free: zero || preset.quirks.model_id.is_free_variant(id),
                context: info.and_then(|i| i.context).or(s.and_then(|s| s.context)),
                tools: s.map(|s| s.tools),
                vision: s.map(|s| s.vision),
                reasoning: s.map(|s| s.reasoning),
                released: s.and_then(|s| s.released.clone()),
                live: info.is_some(),
            }
        };
        match self.live.read().unwrap().get(&preset.id) {
            Some(live) => live
                .iter()
                .map(|m| entry(&m.id, Some(m), snap_models.get(&m.id)))
                .collect(),
            None => snap_models.iter().map(|(id, s)| entry(id, None, Some(s))).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::preset::parse_preset;

    fn preset(extra: &str) -> Preset {
        parse_preset(
            "t.json",
            &format!(
                r#"{{"id":"or","name":"OR","icon":"x","protocol":"openai_chat","base_url":"https://x.example/v1",
                "docs_url":"https://x.example","checked":"2026-11-20","free_tier":"free_models","region":"US",
                "catalog_id":"or"{extra}}}"#
            ),
        )
        .unwrap()
    }

    fn snap() -> Snapshot {
        let mut models = BTreeMap::new();
        models.insert(
            "a".into(),
            SnapModel {
                name: "A".into(),
                price_in: Some(1.0),
                price_out: Some(2.0),
                context: Some(8000),
                tools: true,
                ..Default::default()
            },
        );
        models.insert(
            "free-one".into(),
            SnapModel {
                name: "F".into(),
                price_in: Some(0.0),
                price_out: Some(0.0),
                ..Default::default()
            },
        );
        models.insert(
            "only-snap".into(),
            SnapModel {
                name: "S".into(),
                ..Default::default()
            },
        );
        let mut providers = HashMap::new();
        providers.insert(
            "or".into(),
            SnapProvider {
                name: "OR".into(),
                models,
            },
        );
        Snapshot {
            source: "x".into(),
            fetched: "2026-11-20".into(),
            providers,
        }
    }

    #[test]
    fn snapshot_only_when_no_live_list() {
        let c = Catalog::new(snap());
        let m = c.models(&preset(""));
        assert_eq!(m.len(), 3);
        assert!(m.iter().all(|e| !e.live));
    }

    #[test]
    fn merge_precedence_live_wins() {
        let c = Catalog::new(snap());
        c.set_live(
            "or",
            vec![
                ModelInfo {
                    id: "a".into(),
                    price_in: Some(0.5),
                    context: None,
                    ..Default::default()
                },
                ModelInfo {
                    id: "brand-new".into(),
                    ..Default::default()
                },
            ],
        );
        let m = c.models(&preset(""));
        assert_eq!(m.len(), 2, "live list replaces the snapshot list");
        let a = m.iter().find(|e| e.id == "a").unwrap();
        assert_eq!(a.price_in, Some(0.5), "live price wins");
        assert_eq!(a.price_out, Some(2.0), "snapshot fills gaps");
        assert_eq!(a.context, Some(8000));
        assert_eq!(a.tools, Some(true));
        let n = m.iter().find(|e| e.id == "brand-new").unwrap();
        assert!(!n.pricing_known, "unknown pricing is marked");
        assert!(!n.free);
        assert_eq!(n.tools, None);
    }

    #[test]
    fn free_detection() {
        let c = Catalog::new(snap());
        let p = preset(r#","quirks":{"model_id":{"free_suffix":":free"}}"#);
        c.set_live(
            "or",
            vec![
                ModelInfo {
                    id: "free-one".into(),
                    ..Default::default()
                },
                ModelInfo {
                    id: "x:free".into(),
                    ..Default::default()
                },
                ModelInfo {
                    id: "a".into(),
                    ..Default::default()
                },
            ],
        );
        let m = c.models(&p);
        let free = |id: &str| m.iter().find(|e| e.id == id).unwrap().free;
        assert!(free("free-one"), "price_in == 0 && price_out == 0");
        assert!(free("x:free"), ":free suffix");
        assert!(!free("a"));
    }

    #[test]
    fn local_models_are_free() {
        let c = Catalog::new(Snapshot::default());
        let mut p = preset("");
        p.free_tier = FreeTier::Local;
        c.set_live(
            "or",
            vec![ModelInfo {
                id: "llama3".into(),
                ..Default::default()
            }],
        );
        let m = c.models(&p);
        assert!(m[0].free && m[0].pricing_known);
    }

    #[test]
    fn models_dev_transform_and_bundled_snapshot() {
        let raw = serde_json::json!({"groq":{"name":"Groq","models":{"m":{"name":"M","cost":{"input":0,"output":0},
            "limit":{"context":4096},"tool_call":true,"modalities":{"input":["text","image"]},"release_date":"2025-01-01"}}}});
        let s = Snapshot::from_models_dev(&raw, "2026-11-20").unwrap();
        let m = &s.providers["groq"].models["m"];
        assert!(m.vision && m.tools);
        assert_eq!(m.context, Some(4096));
        assert!(Snapshot::from_models_dev(&serde_json::json!({}), "x").is_err());

        let bundled = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join(SNAPSHOT_FILE);
        let c = Catalog::load(&bundled, Path::new("does-not-exist"));
        assert!(!c.snapshot_date().is_empty(), "bundled snapshot loads");
    }
}
