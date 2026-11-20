//! Rate-limit header parsing. Feeds the "free headroom" bars in the picker.

use super::quirks::RateLimitHeaders;
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitInfo {
    #[specta(type = Option<f64>)]
    pub requests_limit: Option<u64>,
    #[specta(type = Option<f64>)]
    pub requests_remaining: Option<u64>,
    #[specta(type = Option<f64>)]
    pub tokens_limit: Option<u64>,
    #[specta(type = Option<f64>)]
    pub tokens_remaining: Option<u64>,
    /// Seconds until the request budget resets, when given.
    pub reset_secs: Option<f64>,
}

impl RateLimitInfo {
    pub fn is_empty(&self) -> bool {
        *self == RateLimitInfo::default()
    }

    /// Fraction of the tightest known budget that is left, 0.0 to 1.0.
    pub fn headroom(&self) -> Option<f64> {
        let frac = |rem: Option<u64>, lim: Option<u64>| match (rem, lim) {
            (Some(r), Some(l)) if l > 0 => Some((r as f64 / l as f64).clamp(0.0, 1.0)),
            _ => None,
        };
        [
            frac(self.requests_remaining, self.requests_limit),
            frac(self.tokens_remaining, self.tokens_limit),
        ]
        .into_iter()
        .flatten()
        .reduce(f64::min)
    }
}

fn num(h: &HeaderMap, name: &str) -> Option<u64> {
    h.get(name)?
        .to_str()
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| *v >= 0.0)
        .map(|v| v as u64)
}

/// Durations like `1m30s`, `6.5s`, `250ms`, a bare number of seconds, or an
/// RFC 3339 timestamp (Anthropic) / epoch millis (OpenRouter).
pub fn parse_duration_secs(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(v) = s.parse::<f64>() {
        // Epoch millis are far in the future when read as seconds.
        if v > 1.0e11 {
            let now = chrono::Utc::now().timestamp_millis() as f64;
            return Some(((v - now) / 1000.0).max(0.0));
        }
        return Some(v.max(0.0));
    }
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        let secs = (t.timestamp_millis() - chrono::Utc::now().timestamp_millis()) as f64 / 1000.0;
        return Some(secs.max(0.0));
    }
    let mut total = 0.0;
    let mut numbuf = String::new();
    let mut chars = s.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            numbuf.push(c);
            continue;
        }
        let v: f64 = numbuf.parse().ok()?;
        numbuf.clear();
        let unit = match c {
            'h' => 3600.0,
            'm' if chars.peek() == Some(&'s') => {
                chars.next();
                0.001
            }
            'm' => 60.0,
            's' => 1.0,
            _ => return None,
        };
        total += v * unit;
        any = true;
    }
    if !numbuf.is_empty() {
        return None;
    }
    any.then_some(total)
}

pub fn retry_after(h: &HeaderMap) -> Option<u64> {
    let raw = h.get("retry-after")?.to_str().ok()?;
    parse_duration_secs(raw).map(|s| s.ceil() as u64)
}

pub fn parse(style: RateLimitHeaders, h: &HeaderMap) -> RateLimitInfo {
    let reset = |name: &str| h.get(name).and_then(|v| v.to_str().ok()).and_then(parse_duration_secs);
    match style {
        RateLimitHeaders::Openai => RateLimitInfo {
            requests_limit: num(h, "x-ratelimit-limit-requests"),
            requests_remaining: num(h, "x-ratelimit-remaining-requests"),
            tokens_limit: num(h, "x-ratelimit-limit-tokens"),
            tokens_remaining: num(h, "x-ratelimit-remaining-tokens"),
            reset_secs: reset("x-ratelimit-reset-requests"),
        },
        RateLimitHeaders::Anthropic => RateLimitInfo {
            requests_limit: num(h, "anthropic-ratelimit-requests-limit"),
            requests_remaining: num(h, "anthropic-ratelimit-requests-remaining"),
            tokens_limit: num(h, "anthropic-ratelimit-tokens-limit"),
            tokens_remaining: num(h, "anthropic-ratelimit-tokens-remaining"),
            reset_secs: reset("anthropic-ratelimit-requests-reset"),
        },
        RateLimitHeaders::Simple => RateLimitInfo {
            requests_limit: num(h, "x-ratelimit-limit"),
            requests_remaining: num(h, "x-ratelimit-remaining"),
            reset_secs: reset("x-ratelimit-reset"),
            ..Default::default()
        },
        RateLimitHeaders::None => RateLimitInfo::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderName, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn openai_style() {
        let h = headers(&[
            ("x-ratelimit-limit-requests", "14400"),
            ("x-ratelimit-remaining-requests", "14370"),
            ("x-ratelimit-limit-tokens", "6000"),
            ("x-ratelimit-remaining-tokens", "1500"),
            ("x-ratelimit-reset-requests", "2m59.56s"),
        ]);
        let info = parse(RateLimitHeaders::Openai, &h);
        assert_eq!(info.requests_remaining, Some(14370));
        assert!((info.reset_secs.unwrap() - 179.56).abs() < 0.01);
        assert_eq!(info.headroom(), Some(0.25));
    }

    #[test]
    fn anthropic_style() {
        let h = headers(&[
            ("anthropic-ratelimit-requests-limit", "50"),
            ("anthropic-ratelimit-requests-remaining", "49"),
        ]);
        let info = parse(RateLimitHeaders::Anthropic, &h);
        assert_eq!(info.requests_limit, Some(50));
        assert_eq!(info.headroom(), Some(0.98));
    }

    #[test]
    fn missing_headers_mean_unknown() {
        let info = parse(RateLimitHeaders::Openai, &HeaderMap::new());
        assert!(info.is_empty());
        assert_eq!(info.headroom(), None);
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration_secs("7"), Some(7.0));
        assert_eq!(parse_duration_secs("1h2m3s"), Some(3723.0));
        assert_eq!(parse_duration_secs("250ms"), Some(0.25));
        assert_eq!(parse_duration_secs("abc"), None);
        assert_eq!(parse_duration_secs("5x"), None);
        assert_eq!(retry_after(&headers(&[("retry-after", "2.1")])), Some(3));
    }
}
