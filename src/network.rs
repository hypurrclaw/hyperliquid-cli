//! The Hyperliquid network a run targets.

use hypersdk::hypercore::{Chain, HttpClient};

use crate::errors::CliError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    #[default]
    Mainnet,
    Testnet,
}

impl Network {
    pub const VARIANTS: [&'static str; 2] = ["mainnet", "testnet"];

    #[must_use]
    pub fn from_testnet(testnet: bool) -> Self {
        if testnet {
            Self::Testnet
        } else {
            Self::Mainnet
        }
    }

    #[must_use]
    pub fn is_testnet(self) -> bool {
        self == Self::Testnet
    }

    #[must_use]
    pub fn chain(self) -> Chain {
        match self {
            Self::Mainnet => Chain::Mainnet,
            Self::Testnet => Chain::Testnet,
        }
    }

    /// An info/exchange client for this network, pointed at `api_override` when given.
    #[must_use]
    pub fn http_client(self, api_override: Option<reqwest::Url>) -> HttpClient {
        let client = HttpClient::new(self.chain());
        match api_override {
            Some(url) => client.with_url(url),
            None => client,
        }
    }

    /// The public API endpoint, before any loopback test override.
    #[must_use]
    pub fn api_base_url(self) -> &'static str {
        match self {
            Self::Mainnet => "https://api.hyperliquid.xyz",
            Self::Testnet => "https://api.hyperliquid-testnet.xyz",
        }
    }

    /// The public API endpoint as a URL, before any loopback test override.
    #[must_use]
    pub fn api_url(self) -> reqwest::Url {
        match self {
            Self::Mainnet => hypersdk::hypercore::mainnet_url(),
            Self::Testnet => hypersdk::hypercore::testnet_url(),
        }
    }

    /// Case-insensitive parse shared by the config file and `HYPERLIQUID_NETWORK`.
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_lowercase().as_str() {
            "mainnet" => Some(Self::Mainnet),
            "testnet" => Some(Self::Testnet),
            _ => None,
        }
    }

    /// Parse a setting value, naming `source` in the error.
    pub fn parse_setting(source: &str, value: &str) -> Result<Self, CliError> {
        Self::parse(value).ok_or_else(|| {
            CliError::Configuration(format!(
                "{source} must be either 'mainnet' or 'testnet' (got '{value}')"
            ))
        })
    }
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mainnet => write!(f, "mainnet"),
            Self::Testnet => write!(f, "testnet"),
        }
    }
}

impl<'de> serde::Deserialize<'de> for Network {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| {
            serde::de::Error::unknown_variant(&value.to_lowercase(), &Self::VARIANTS)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_default() {
        assert_eq!(Network::Mainnet.to_string(), "mainnet");
        assert_eq!(Network::Testnet.to_string(), "testnet");
        assert_eq!(Network::default(), Network::Mainnet);
    }

    #[test]
    fn serde_roundtrip_is_lowercase_and_case_insensitive() {
        assert_eq!(
            serde_json::to_string(&Network::Testnet).unwrap(),
            "\"testnet\""
        );
        let parsed: Network = serde_json::from_str("\"TESTNET\"").unwrap();
        assert_eq!(parsed, Network::Testnet);
        let parsed: Network = serde_json::from_str("\"Mainnet\"").unwrap();
        assert_eq!(parsed, Network::Mainnet);
        assert!(serde_json::from_str::<Network>("\"bogus\"").is_err());
    }

    #[test]
    fn chain_and_url_mapping() {
        assert!(Network::Mainnet.chain().is_mainnet());
        assert!(!Network::Testnet.chain().is_mainnet());
        assert_eq!(
            Network::Mainnet.api_base_url(),
            "https://api.hyperliquid.xyz"
        );
        assert_eq!(
            Network::Testnet.api_base_url(),
            "https://api.hyperliquid-testnet.xyz"
        );
    }

    #[test]
    fn setting_error_names_source() {
        let err = Network::parse_setting("HYPERLIQUID_NETWORK", "bogus").unwrap_err();
        assert!(err.to_string().contains("HYPERLIQUID_NETWORK"));
        assert!(err.to_string().contains("'bogus'"));
    }
}
