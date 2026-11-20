//! Per-provider differences, declared as data in each preset instead of
//! `if provider == ...` branches in the adapters.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", deny_unknown_fields, default)]
pub struct Quirks {
    /// Name of the output token cap field in the request body.
    pub max_tokens_field: MaxTokensField,
    pub temperature: TempRule,
    /// Where reasoning text comes back in a streamed response.
    pub reasoning: ReasoningLoc,
    /// Send previous reasoning back to the model on the next turn.
    pub send_reasoning_back: bool,
    pub tools: ToolSupport,
    /// Ask for a usage chunk at the end of a stream (`stream_options.include_usage`).
    pub stream_usage: bool,
    /// Dotted path to a cost value (USD) in the usage object, e.g. `cost`.
    pub cost_field: Option<String>,
    /// Extra headers sent on every request. Values must not contain secrets.
    pub extra_headers: BTreeMap<String, String>,
    pub user_agent: Option<String>,
    pub ratelimit_headers: RateLimitHeaders,
    pub images: ImageSupport,
    pub model_id: ModelIdRule,
    /// Value of the `anthropic-version` header (Anthropic protocol only).
    pub anthropic_version: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MaxTokensField {
    #[default]
    MaxTokens,
    MaxCompletionTokens,
}

impl MaxTokensField {
    pub fn key(self) -> &'static str {
        match self {
            MaxTokensField::MaxTokens => "max_tokens",
            MaxTokensField::MaxCompletionTokens => "max_completion_tokens",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", tag = "rule")]
pub enum TempRule {
    /// Send the requested temperature as is.
    #[default]
    Pass,
    /// Never send a temperature (the provider rejects it).
    Omit,
    /// Clamp into the provider's accepted range.
    Clamp { min: f32, max: f32 },
}

impl TempRule {
    pub fn apply(self, requested: Option<f32>) -> Option<f32> {
        match (self, requested) {
            (_, None) | (TempRule::Omit, _) => None,
            (TempRule::Pass, Some(t)) => Some(t),
            (TempRule::Clamp { min, max }, Some(t)) => Some(t.clamp(min, max)),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningLoc {
    /// Read `reasoning_content` and `reasoning` fields and `<think>` tags.
    #[default]
    Auto,
    ReasoningContent,
    Reasoning,
    ThinkTags,
    None,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ToolSupport {
    #[default]
    Native,
    /// Native tools, but only one call per turn.
    NoParallel,
    /// No native tools: use the JSON-in-text protocol.
    None,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitHeaders {
    /// `x-ratelimit-{limit,remaining,reset}-{requests,tokens}`
    #[default]
    Openai,
    /// `anthropic-ratelimit-{requests,tokens}-{limit,remaining,reset}`
    Anthropic,
    /// `x-ratelimit-{limit,remaining,reset}` (single bucket)
    Simple,
    None,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ImageSupport {
    #[default]
    None,
    /// Base64 data URLs in the provider's native image part.
    Native,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", deny_unknown_fields, default)]
pub struct ModelIdRule {
    /// Suffix that marks a free model variant, e.g. `:free`. Kept as part of the id.
    pub free_suffix: Option<String>,
    /// Model ids look like `vendor/model`.
    pub vendor_prefix: bool,
}

impl ModelIdRule {
    pub fn is_free_variant(&self, model_id: &str) -> bool {
        self.free_suffix.as_deref().is_some_and(|s| model_id.ends_with(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_rules() {
        assert_eq!(TempRule::Pass.apply(Some(1.5)), Some(1.5));
        assert_eq!(TempRule::Omit.apply(Some(0.2)), None);
        assert_eq!(TempRule::Clamp { min: 0.0, max: 1.0 }.apply(Some(1.7)), Some(1.0));
        assert_eq!(TempRule::Clamp { min: 0.0, max: 1.0 }.apply(None), None);
    }

    #[test]
    fn free_suffix_detection() {
        let rule = ModelIdRule { free_suffix: Some(":free".into()), vendor_prefix: true };
        assert!(rule.is_free_variant("meta-llama/llama-3.3-70b-instruct:free"));
        assert!(!rule.is_free_variant("meta-llama/llama-3.3-70b-instruct"));
        assert!(!ModelIdRule::default().is_free_variant("x:free"));
    }

    #[test]
    fn empty_quirks_parse_to_defaults() {
        let q: Quirks = serde_json::from_str("{}").unwrap();
        assert_eq!(q, Quirks::default());
        let q: Quirks = serde_json::from_str(r#"{"temperature":{"rule":"clamp","min":0,"max":1}}"#).unwrap();
        assert_eq!(q.temperature, TempRule::Clamp { min: 0.0, max: 1.0 });
    }

    #[test]
    fn unknown_quirk_is_rejected() {
        assert!(serde_json::from_str::<Quirks>(r#"{"max_tokenz":"x"}"#).is_err());
    }
}
