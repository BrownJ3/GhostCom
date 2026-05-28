use std::collections::HashMap;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

const ACCESS_TOKEN_ENV: &str = "GHSTCOM_RELAY_ACCESS_TOKEN";
const ALLOWED_DEVICE_KEYS_ENV: &str = "GHSTCOM_RELAY_ALLOWED_DEVICE_KEYS";
const RELAY_ENABLED_ENV: &str = "GHSTCOM_RELAY_ENABLED";
const RENDEZVOUS_ENABLED_ENV: &str = "GHSTCOM_RENDEZVOUS_ENABLED";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SiteConfig {
    pub relay_enabled: bool,
    pub rendezvous_enabled: bool,
    access_token: Option<String>,
    allowed_device_keys: HashMap<String, Option<u64>>,
}

impl SiteConfig {
    pub fn from_env() -> Self {
        Self {
            relay_enabled: bool_env(RELAY_ENABLED_ENV, true),
            rendezvous_enabled: bool_env(RENDEZVOUS_ENABLED_ENV, false),
            access_token: env::var(ACCESS_TOKEN_ENV)
                .ok()
                .map(|token| token.trim().to_string())
                .filter(|token| !token.is_empty()),
            allowed_device_keys: parse_allowed_device_keys(
                env::var(ALLOWED_DEVICE_KEYS_ENV).ok().as_deref(),
            ),
        }
    }

    pub fn token_matches(&self, supplied: Option<&str>) -> bool {
        match (self.access_token.as_deref(), supplied) {
            (None, _) => true,
            (Some(expected), Some(supplied)) => {
                constant_time_eq(expected.as_bytes(), supplied.as_bytes())
            }
            (Some(_), None) => false,
        }
    }

    pub fn device_key_allowed(&self, public_key: &str) -> bool {
        if self.allowed_device_keys.is_empty() {
            return true;
        }

        match self.allowed_device_keys.get(public_key) {
            Some(Some(expires_at)) => current_unix_time() <= *expires_at,
            Some(None) => true,
            None => false,
        }
    }

    pub fn requires_device_key(&self) -> bool {
        !self.allowed_device_keys.is_empty()
    }

    #[cfg(test)]
    pub fn for_tests(
        relay_enabled: bool,
        rendezvous_enabled: bool,
        access_token: Option<&str>,
    ) -> Self {
        Self {
            relay_enabled,
            rendezvous_enabled,
            access_token: access_token.map(str::to_string),
            allowed_device_keys: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub fn for_tests_with_devices(
        relay_enabled: bool,
        rendezvous_enabled: bool,
        access_token: Option<&str>,
        allowed_device_keys: &[&str],
    ) -> Self {
        Self {
            relay_enabled,
            rendezvous_enabled,
            access_token: access_token.map(str::to_string),
            allowed_device_keys: allowed_device_keys
                .iter()
                .filter_map(|entry| parse_allowed_device_entry(entry))
                .collect(),
        }
    }
}

impl Default for SiteConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

fn bool_env(name: &str, default: bool) -> bool {
    match env::var(name) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut diff = left.len() ^ right.len();

    for index in 0..max_len {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        diff |= (a ^ b) as usize;
    }

    diff == 0
}

fn parse_allowed_device_keys(raw: Option<&str>) -> HashMap<String, Option<u64>> {
    raw.unwrap_or("")
        .split(|ch: char| ch == ',' || ch == ';' || ch.is_whitespace())
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .filter_map(parse_allowed_device_entry)
        .collect()
}

fn parse_allowed_device_entry(entry: &str) -> Option<(String, Option<u64>)> {
    let (key, expires_at) = match entry.split_once('@') {
        Some((key, expires_at)) => (key.trim(), Some(expires_at.trim().parse().ok()?)),
        None => (entry.trim(), None),
    };

    if key.is_empty() {
        return None;
    }

    Some((key.to_string(), expires_at))
}

fn current_unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_matching_is_exact() {
        let config = SiteConfig {
            relay_enabled: true,
            rendezvous_enabled: true,
            access_token: Some("secret-token".to_string()),
            allowed_device_keys: HashMap::new(),
        };

        assert!(config.token_matches(Some("secret-token")));
        assert!(!config.token_matches(Some("secret")));
        assert!(!config.token_matches(None));
    }

    #[test]
    fn missing_token_allows_public_mode() {
        let config = SiteConfig {
            relay_enabled: true,
            rendezvous_enabled: true,
            access_token: None,
            allowed_device_keys: HashMap::new(),
        };

        assert!(config.token_matches(None));
        assert!(config.token_matches(Some("anything")));
    }

    #[test]
    fn parses_allowed_device_keys() {
        let parsed = parse_allowed_device_keys(Some("abc, def\nghi"));

        assert!(parsed.contains_key("abc"));
        assert!(parsed.contains_key("def"));
        assert!(parsed.contains_key("ghi"));
    }

    #[test]
    fn parses_expiring_device_keys() {
        let parsed = parse_allowed_device_keys(Some("abc@4102444800"));

        assert_eq!(parsed.get("abc"), Some(&Some(4102444800)));
    }

    #[test]
    fn expired_device_key_is_rejected() {
        let config = SiteConfig::for_tests_with_devices(true, false, None, &["abc@1"]);

        assert!(!config.device_key_allowed("abc"));
    }
}
