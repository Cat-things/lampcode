use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    System,
}

/// Answer to the one-time first-launch question about update checks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum UpdateCheck {
    #[default]
    NotAsked,
    Enabled,
    Disabled,
}

/// App settings. Never holds secrets: keys live in the key store.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: Theme,
    pub update_check: UpdateCheck,
    /// Provider ids in the order the user dragged them in the rail.
    pub provider_order: Vec<String>,
    pub disabled_providers: Vec<String>,
    /// Starred models as `provider_id/model_id`.
    pub favorite_models: Vec<String>,
    pub last_model: Option<ModelRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub provider_id: String,
    pub model_id: String,
}
