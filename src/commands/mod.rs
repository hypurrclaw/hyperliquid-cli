//! Command modules.

pub mod account;
pub(crate) mod actions;
pub mod api_wallet;
pub mod asset;
pub mod borrowlend;
pub mod builder;
pub mod feedback;
pub mod meta;
pub mod orderbook;
pub mod orders;
pub mod outcomes;
pub mod perps;
pub mod positions;
pub mod prio;
pub mod referral;
pub mod schema;
pub mod setup;
pub mod spot;
pub(crate) mod spot_balances;
pub mod staking;
pub mod status;
pub mod subaccounts;
pub mod transfers;
pub mod vaults;
pub mod wallet;
pub mod watch_risk;

use crate::assets::AssetError;
use crate::errors::CliError;

impl From<AssetError> for CliError {
    fn from(err: AssetError) -> Self {
        match err {
            AssetError::NotFound {
                asset,
                suggestion: Some(suggestion),
            } => CliError::AssetNotFound {
                asset,
                suggestions: suggestion,
            },
            AssetError::NotFound {
                asset,
                suggestion: None,
            } => CliError::AssetNotFoundNoSuggestion { asset },
            AssetError::UnsupportedOutcome { input } => {
                outcomes::unsupported_outcome_trading_error(&input)
            }
            AssetError::Load(err) => err,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_errors_convert_to_their_cli_errors() {
        let not_found = CliError::from(AssetError::NotFound {
            asset: "BTCC".to_string(),
            suggestion: Some("BTC".to_string()),
        });
        assert_eq!(not_found.exit_code(), 13);
        assert_eq!(
            not_found.to_string(),
            "Couldn't find asset \"BTCC\". Did you mean BTC?"
        );
        assert_eq!(not_found.suggestion(), Some("BTC"));

        let bare = CliError::from(AssetError::NotFound {
            asset: "ZZZ".to_string(),
            suggestion: None,
        });
        assert!(matches!(bare, CliError::AssetNotFoundNoSuggestion { .. }));

        let outcome = CliError::from(AssetError::UnsupportedOutcome {
            input: "#10".to_string(),
        });
        assert_eq!(outcome.exit_code(), 13);
        assert!(outcome.to_string().contains("outcome-aware command path"));
        assert!(outcome.to_string().contains("outcomes get #10"));

        let load = CliError::from(AssetError::Load(CliError::AuthRequired));
        assert!(matches!(load, CliError::AuthRequired));
    }
}
