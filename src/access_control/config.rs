//! Device-side access-control settings, parsed once per use from the option
//! store so that a change in Settings takes effect on the next connection.

use hbb_common::config::Config;
use base::config::keys;

pub const DEFAULT_HEARTBEAT_SECS: u64 = 30;
pub const DEFAULT_CACHE_DAYS: u64 = 7;
/// Below this the backend would be hammered; the UI also refuses smaller values.
pub const MIN_HEARTBEAT_SECS: u64 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcConfig {
    /// Master switch. When false the device behaves like stock RustDesk.
    pub enabled: bool,
    /// Backend base URL without a trailing slash, e.g. `https://access.example.com`.
    pub api_url: String,
    /// Sent as `Authorization: Bearer <api_key>`.
    pub api_key: String,
    pub heartbeat_secs: u64,
    pub cache_days: u64,
}

impl AcConfig {
    /// Reads the device-scope options.
    pub fn load() -> Self {
        Self::from_options(|k| Config::get_option(k))
    }

    /// Builds a config from any option source; the seam the tests use.
    pub fn from_options(get: impl Fn(&str) -> String) -> Self {
        Self {
            enabled: get(keys::OPTION_ACCESS_CONTROL) == "Y",
            api_url: normalize_url(&get(keys::OPTION_ACCESS_API_URL)),
            api_key: get(keys::OPTION_ACCESS_API_KEY).trim().to_owned(),
            heartbeat_secs: parse_or(&get(keys::OPTION_ACCESS_HEARTBEAT_SECS), DEFAULT_HEARTBEAT_SECS)
                .max(MIN_HEARTBEAT_SECS),
            cache_days: parse_or(&get(keys::OPTION_ACCESS_CACHE_DAYS), DEFAULT_CACHE_DAYS),
        }
    }

    /// Enabled *and* has what it needs to call the backend. An enabled but
    /// unconfigured device refuses incoming connections rather than falling
    /// back to password-only (plan §3.1 step a).
    pub fn is_configured(&self) -> bool {
        self.enabled && !self.api_url.is_empty() && !self.api_key.is_empty()
    }
}

fn normalize_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_owned()
}

fn parse_or(raw: &str, default: u64) -> u64 {
    raw.trim().parse().unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> AcConfig {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        AcConfig::from_options(|k| map.get(k).cloned().unwrap_or_default())
    }

    #[test]
    fn defaults_when_nothing_is_set() {
        let c = cfg(&[]);
        assert!(!c.enabled);
        assert_eq!(c.api_url, "");
        assert_eq!(c.api_key, "");
        assert_eq!(c.heartbeat_secs, DEFAULT_HEARTBEAT_SECS);
        assert_eq!(c.cache_days, DEFAULT_CACHE_DAYS);
        assert!(!c.is_configured());
    }

    #[test]
    fn enabled_requires_url_and_key_to_be_configured() {
        assert!(!cfg(&[("access-control", "Y")]).is_configured());
        assert!(!cfg(&[("access-control", "Y"), ("access-api-url", "https://a")]).is_configured());
        assert!(!cfg(&[("access-control", "Y"), ("access-api-key", "k")]).is_configured());
        assert!(cfg(&[
            ("access-control", "Y"),
            ("access-api-url", "https://a"),
            ("access-api-key", "k"),
        ])
        .is_configured());
        // Configured values without the switch stay inert.
        assert!(!cfg(&[("access-api-url", "https://a"), ("access-api-key", "k")]).is_configured());
    }

    #[test]
    fn url_is_trimmed_and_loses_trailing_slashes() {
        let c = cfg(&[("access-api-url", "  https://access.example.com//  ")]);
        assert_eq!(c.api_url, "https://access.example.com");
    }

    #[test]
    fn whitespace_only_key_counts_as_missing() {
        let c = cfg(&[("access-control", "Y"), ("access-api-url", "https://a"), ("access-api-key", "   ")]);
        assert!(!c.is_configured());
    }

    #[test]
    fn numbers_fall_back_to_defaults_and_heartbeat_has_a_floor() {
        assert_eq!(cfg(&[("access-heartbeat-secs", "abc")]).heartbeat_secs, DEFAULT_HEARTBEAT_SECS);
        assert_eq!(cfg(&[("access-heartbeat-secs", "60")]).heartbeat_secs, 60);
        assert_eq!(cfg(&[("access-heartbeat-secs", "1")]).heartbeat_secs, MIN_HEARTBEAT_SECS);
        assert_eq!(cfg(&[("access-cache-days", "-3")]).cache_days, DEFAULT_CACHE_DAYS);
        assert_eq!(cfg(&[("access-cache-days", "14")]).cache_days, 14);
    }
}
