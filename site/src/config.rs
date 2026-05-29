use std::env;
use subtle::ConstantTimeEq;

const ACCESS_TOKEN_ENV: &str = "GHSTCOM_RELAY_ACCESS_TOKEN";
const RELAY_ENABLED_ENV: &str = "GHSTCOM_RELAY_ENABLED";
const RENDEZVOUS_ENABLED_ENV: &str = "GHSTCOM_RENDEZVOUS_ENABLED";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SiteConfig {
    pub relay_enabled: bool,
    pub rendezvous_enabled: bool,
    access_token: Option<String>,
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
        }
    }

    pub fn token_matches(&self, supplied: Option<&str>) -> bool {
        match (self.access_token.as_deref(), supplied) {
            (None, _) => true,
            (Some(expected), Some(supplied)) => {
                bool::from(expected.as_bytes().ct_eq(supplied.as_bytes()))
            }
            (Some(_), None) => false,
        }
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_matching_is_exact() {
        let config = SiteConfig {
            relay_enabled: true,
            rendezvous_enabled: true,
            access_token: Some("secret-token".to_string()),
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
        };

        assert!(config.token_matches(None));
        assert!(config.token_matches(Some("anything")));
    }
}
