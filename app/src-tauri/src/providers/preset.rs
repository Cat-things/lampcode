//! Provider presets. One JSON file per provider; a user "provider pack" is the
//! same format, so built-ins and packs share one loader.

use super::quirks::Quirks;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    OpenaiChat,
    Anthropic,
    Ollama,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FreeTier {
    None,
    FreeTier,
    FreeModels,
    TrialCredits,
    Local,
    /// The official docs don't state it.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Tag {
    Free,
    Eu,
    Local,
    Router,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Variant {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub region: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Preset {
    pub id: String,
    pub name: String,
    /// Icon id from the app's icon set (never a remote URL).
    pub icon: String,
    pub protocol: Protocol,
    pub base_url: String,
    /// Env var that overrides a saved key.
    #[serde(default)]
    pub key_env: Option<String>,
    /// False for local servers that take no key.
    #[serde(default = "yes")]
    pub key_required: bool,
    #[serde(default)]
    pub signup_url: Option<String>,
    pub docs_url: String,
    /// Date the official docs were checked, `YYYY-MM-DD`.
    pub checked: String,
    pub free_tier: FreeTier,
    #[serde(default)]
    pub free_tier_note: String,
    pub region: String,
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub variants: Vec<Variant>,
    /// Provider id in the bundled models.dev snapshot, if it is listed there.
    #[serde(default)]
    pub catalog_id: Option<String>,
    #[serde(default)]
    pub quirks: Quirks,
}

fn yes() -> bool {
    true
}

impl Preset {
    pub fn host(&self) -> Option<String> {
        url::Url::parse(&self.base_url)
            .ok()?
            .host_str()
            .map(str::to_ascii_lowercase)
    }

    fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(format!("id {:?} must be lowercase letters, digits and dashes", self.id));
        }
        if chrono::NaiveDate::parse_from_str(&self.checked, "%Y-%m-%d").is_err() {
            return Err(format!("checked {:?} is not a YYYY-MM-DD date", self.checked));
        }
        if !self.docs_url.starts_with("https://") {
            return Err("docs_url must be an https URL".into());
        }
        for u in std::iter::once(&self.base_url).chain(self.variants.iter().map(|v| &v.base_url)) {
            let parsed = url::Url::parse(u).map_err(|e| format!("base_url {u:?}: {e}"))?;
            let local = matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
            if parsed.scheme() != "https" && !(parsed.scheme() == "http" && local) {
                return Err(format!("base_url {u:?} must be https (plain http only for localhost)"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PresetError {
    #[error("{file}: {msg}")]
    Invalid { file: String, msg: String },
    #[error("duplicate provider id {id:?} in {first} and {second}")]
    Duplicate { id: String, first: String, second: String },
    #[error("could not read {0}")]
    Io(String),
}

#[derive(Debug, Default)]
pub struct PresetRegistry {
    presets: BTreeMap<String, (Preset, Origin)>,
    /// Problems with user packs. They never stop the app from starting.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Builtin,
    User,
}

fn json_files(dir: &Path) -> Result<Vec<PathBuf>, PresetError> {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(PresetError::Io(format!("{}: {e}", dir.display()))),
    };
    let mut files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    Ok(files)
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn parse_preset(name: &str, text: &str) -> Result<Preset, PresetError> {
    let preset: Preset = serde_json::from_str(text).map_err(|e| PresetError::Invalid {
        file: name.into(),
        msg: e.to_string(),
    })?;
    preset
        .validate()
        .map_err(|msg| PresetError::Invalid { file: name.into(), msg })?;
    Ok(preset)
}

impl PresetRegistry {
    /// Built-in presets must all be valid and unique (that is a bug in the
    /// app). User packs are best effort: bad or shadowing files are skipped
    /// with a warning.
    pub fn load(builtin_dir: &Path, user_dir: Option<&Path>) -> Result<Self, PresetError> {
        let mut reg = PresetRegistry::default();
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        for path in json_files(builtin_dir)? {
            let name = file_name(&path);
            let text = std::fs::read_to_string(&path).map_err(|e| PresetError::Io(format!("{name}: {e}")))?;
            let preset = parse_preset(&name, &text)?;
            if let Some(first) = seen.insert(preset.id.clone(), name.clone()) {
                return Err(PresetError::Duplicate {
                    id: preset.id,
                    first,
                    second: name,
                });
            }
            reg.presets.insert(preset.id.clone(), (preset, Origin::Builtin));
        }
        if let Some(dir) = user_dir {
            for path in json_files(dir).unwrap_or_default() {
                let name = file_name(&path);
                let parsed = std::fs::read_to_string(&path)
                    .map_err(|e| PresetError::Io(format!("{name}: {e}")))
                    .and_then(|t| parse_preset(&name, &t));
                match parsed {
                    Err(e) => reg.warnings.push(format!("skipped provider pack {e}")),
                    Ok(p) => {
                        if let Some(first) = seen.get(&p.id) {
                            reg.warnings.push(format!(
                                "skipped provider pack {name}: id {:?} is already used by {first}",
                                p.id
                            ));
                        } else {
                            seen.insert(p.id.clone(), name);
                            reg.presets.insert(p.id.clone(), (p, Origin::User));
                        }
                    }
                }
            }
        }
        Ok(reg)
    }

    pub fn get(&self, id: &str) -> Option<&Preset> {
        self.presets.get(id).map(|(p, _)| p)
    }

    pub fn origin(&self, id: &str) -> Option<Origin> {
        self.presets.get(id).map(|(_, o)| *o)
    }

    pub fn all(&self) -> impl Iterator<Item = &Preset> {
        self.presets.values().map(|(p, _)| p)
    }

    #[cfg(test)]
    pub fn from_presets(presets: Vec<Preset>) -> Self {
        let mut reg = PresetRegistry::default();
        for p in presets {
            reg.presets.insert(p.id.clone(), (p, Origin::Builtin));
        }
        reg
    }
}

/// Directory of the bundled presets when running from the source tree.
pub fn source_builtin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("providers")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    fn minimal(id: &str) -> String {
        format!(
            r#"{{"id":"{id}","name":"X","icon":"x","protocol":"openai_chat","base_url":"https://api.example.com/v1",
            "docs_url":"https://example.com/docs","checked":"2026-11-20","free_tier":"none","region":"US"}}"#
        )
    }

    #[test]
    fn every_bundled_preset_parses_and_has_docs_url_and_checked_date() {
        let reg = PresetRegistry::load(&source_builtin_dir(), None).expect("bundled presets load");
        assert!(reg.all().count() >= 8, "expected the first batch of presets");
        for p in reg.all() {
            assert!(p.docs_url.starts_with("https://"), "{}: docs_url", p.id);
            assert!(
                chrono::NaiveDate::parse_from_str(&p.checked, "%Y-%m-%d").is_ok(),
                "{}: checked",
                p.id
            );
            assert!(p.host().is_some(), "{}: base_url host", p.id);
            if p.key_required {
                assert!(p.key_env.is_some(), "{}: keyed provider needs key_env", p.id);
            }
        }
    }

    #[test]
    fn bundled_file_name_matches_id() {
        for path in json_files(&source_builtin_dir()).unwrap() {
            let p = parse_preset(&file_name(&path), &std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(file_name(&path), format!("{}.json", p.id));
        }
    }

    #[test]
    fn duplicate_builtin_ids_name_both_files() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.json", &minimal("same"));
        write(dir.path(), "b.json", &minimal("same"));
        let err = PresetRegistry::load(dir.path(), None).unwrap_err();
        assert_eq!(
            err,
            PresetError::Duplicate {
                id: "same".into(),
                first: "a.json".into(),
                second: "b.json".into()
            }
        );
        let msg = err.to_string();
        assert!(msg.contains("a.json") && msg.contains("b.json"));
    }

    #[test]
    fn user_pack_cannot_shadow_builtin() {
        let builtin = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        write(builtin.path(), "groq.json", &minimal("groq"));
        write(
            user.path(),
            "evil.json",
            &minimal("groq").replace("api.example.com", "evil.example.net"),
        );
        write(user.path(), "mine.json", &minimal("mine"));
        let reg = PresetRegistry::load(builtin.path(), Some(user.path())).unwrap();
        assert_eq!(reg.get("groq").unwrap().base_url, "https://api.example.com/v1");
        assert_eq!(reg.origin("mine"), Some(Origin::User));
        assert_eq!(reg.warnings.len(), 1);
        assert!(reg.warnings[0].contains("evil.json") && reg.warnings[0].contains("groq.json"));
    }

    #[test]
    fn broken_user_pack_is_a_warning_not_an_error() {
        let builtin = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        write(user.path(), "broken.json", "{not json");
        let reg = PresetRegistry::load(builtin.path(), Some(user.path())).unwrap();
        assert_eq!(reg.all().count(), 0);
        assert!(reg.warnings[0].contains("broken.json"));
    }

    #[test]
    fn plain_http_only_for_localhost() {
        let remote = minimal("a").replace("https://api.example.com/v1", "http://api.example.com/v1");
        assert!(parse_preset("a.json", &remote).is_err());
        let local = minimal("a").replace("https://api.example.com/v1", "http://127.0.0.1:11434");
        assert!(parse_preset("a.json", &local).is_ok());
    }

    #[test]
    fn bad_dates_and_ids_are_rejected() {
        assert!(parse_preset("a.json", &minimal("a").replace("2026-11-20", "Nov 20")).is_err());
        assert!(parse_preset("a.json", &minimal("Bad_Id")).is_err());
        assert!(
            parse_preset(
                "a.json",
                &minimal("a").replace(r#""region":"US""#, r#""region":"US","surprise":1"#)
            )
            .is_err()
        );
    }
}
