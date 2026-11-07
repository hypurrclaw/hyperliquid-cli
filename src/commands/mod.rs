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
pub mod subscribe;
pub(crate) mod support;
pub mod transfers;
pub mod update;
pub mod vaults;
pub mod wallet;
pub mod watch_risk;

use std::sync::LazyLock;

use crate::assets::AssetError;
use crate::command_spec::{Entry, GroupSpec};
use crate::errors::CliError;

/// Every command, in help order. Registering a command is one line here.
pub fn entries() -> &'static [Entry] {
    static ENTRIES: LazyLock<Vec<Entry>> = LazyLock::new(|| {
        vec![
            Entry::of::<perps::PerpsList>(),
            Entry::of::<perps::PerpsGet>(),
            Entry::of::<spot::SpotList>(),
            Entry::of::<spot::SpotGet>(),
            Entry::of::<asset::AssetDecode>(),
            Entry::of::<asset::AssetSearch>(),
            Entry::of::<outcomes::OutcomesList>(),
            Entry::of::<outcomes::OutcomesGet>(),
            Entry::of::<orderbook::Book>(),
            Entry::of::<orderbook::Mids>(),
            Entry::of::<orderbook::Candles>(),
            Entry::of::<orderbook::Spread>(),
            Entry::of::<orderbook::Funding>(),
            Entry::of::<asset::Search>(),
            Entry::of::<status::Status>(),
            Entry::of::<meta::Meta>(),
            Entry::of::<orders::cli::OrdersOpen>(),
            Entry::of::<orders::cli::OrdersStatus>(),
            Entry::of::<orders::cli::OrdersHistory>(),
            Entry::of::<orders::cli::OrdersCreate>(),
            Entry::of::<orders::cli::OrdersScale>(),
            Entry::of::<orders::cli::OrdersBatchCreate>(),
            Entry::of::<orders::cli::OrdersTpsl>(),
            Entry::of::<orders::cli::OrdersTrailingStop>(),
            Entry::of::<orders::cli::OrdersCancel>(),
            Entry::of::<orders::cli::OrdersCancelAll>(),
            Entry::of::<orders::cli::OrdersModify>(),
            Entry::of::<orders::cli::OrdersTwapCreate>(),
            Entry::of::<orders::cli::OrdersTwapCancel>(),
            Entry::of::<orders::cli::OrdersScheduleCancel>(),
            Entry::of::<orders::cli::OrdersChase>(),
            Entry::of::<orders::cli::OrdersBracket>(),
            Entry::of::<orders::cli::Buy>(),
            Entry::of::<orders::cli::Sell>(),
            Entry::of::<orders::cli::OutcomeBuy>(),
            Entry::of::<orders::cli::OutcomeSell>(),
            Entry::of::<positions::PositionsList>(),
            Entry::of::<positions::PositionsUpdateLeverage>(),
            Entry::of::<positions::PositionsUpdateMargin>(),
            Entry::of::<account::AccountFills>(),
            Entry::of::<account::AccountFees>(),
            Entry::of::<account::AccountRateLimit>(),
            Entry::of::<account::AccountOrders>(),
            Entry::of::<account::AccountPortfolio>(),
            Entry::of::<account::AccountSubaccounts>(),
            Entry::of::<account::AccountPortfolioHistory>(),
            Entry::of::<account::AccountLedger>(),
            Entry::of::<account::AccountFunding>(),
            Entry::of::<account::AccountTwapHistory>(),
            Entry::of::<account::AccountTwapFills>(),
            Entry::of::<account::AccountAbstraction>(),
            Entry::of::<account::AccountAbstractionSet>(),
            Entry::of::<wallet::AccountAdd>(),
            Entry::of::<wallet::AccountLs>(),
            Entry::of::<wallet::AccountSetDefault>(),
            Entry::of::<wallet::AccountRemove>(),
            Entry::of::<wallet::WalletCreate>(),
            Entry::of::<wallet::WalletImport>(),
            Entry::of::<wallet::WalletImportMnemonic>(),
            Entry::of::<wallet::WalletShow>(),
            Entry::of::<wallet::WalletAddress>(),
            Entry::of::<wallet::WalletList>(),
            Entry::of::<wallet::WalletRename>(),
            Entry::of::<wallet::WalletDelete>(),
            Entry::of::<wallet::WalletExport>(),
            Entry::of::<wallet::WalletReset>(),
            Entry::of::<api_wallet::ApiWalletCreate>(),
            Entry::of::<api_wallet::ApiWalletApprove>(),
            Entry::of::<api_wallet::ApiWalletList>(),
            Entry::of::<api_wallet::ApiWalletRevoke>(),
            Entry::of::<subaccounts::SubaccountList>(),
            Entry::of::<subaccounts::SubaccountCreate>(),
            Entry::of::<subaccounts::SubaccountTransfer>(),
            Entry::of::<subaccounts::SubaccountSpotTransfer>(),
            Entry::of::<builder::BuilderMaxFee>(),
            Entry::of::<builder::BuilderApproved>(),
            Entry::of::<builder::BuilderApprove>(),
            Entry::of::<referral::ReferralSet>(),
            Entry::of::<referral::ReferralRegister>(),
            Entry::of::<referral::ReferralStatus>(),
            Entry::of::<staking::StakingSummary>(),
            Entry::of::<staking::StakingValidators>(),
            Entry::of::<staking::StakingRewards>(),
            Entry::of::<staking::StakingHistory>(),
            Entry::of::<staking::StakingDelegate>(),
            Entry::of::<staking::StakingUndelegate>(),
            Entry::of::<staking::StakingDeposit>(),
            Entry::of::<staking::StakingWithdraw>(),
            Entry::of::<staking::StakingClaimRewards>(),
            Entry::of::<staking::StakingLinkInitiate>(),
            Entry::of::<staking::StakingLinkFinalize>(),
            Entry::of::<vaults::VaultList>(),
            Entry::of::<vaults::VaultSearch>(),
            Entry::of::<vaults::VaultGet>(),
            Entry::of::<vaults::VaultPositions>(),
            Entry::of::<vaults::VaultDeposit>(),
            Entry::of::<vaults::VaultWithdraw>(),
            Entry::of::<borrowlend::BorrowlendRates>(),
            Entry::of::<borrowlend::BorrowlendGet>(),
            Entry::of::<borrowlend::BorrowlendUser>(),
            Entry::of::<borrowlend::BorrowlendSupply>(),
            Entry::of::<borrowlend::BorrowlendWithdraw>(),
            Entry::of::<prio::PrioStatus>(),
            Entry::of::<prio::PrioBid>(),
            Entry::of::<transfers::TransferSpotToPerp>(),
            Entry::of::<transfers::TransferPerpToSpot>(),
            Entry::of::<transfers::TransferSend>(),
            Entry::of::<transfers::TransferSpotSend>(),
            Entry::of::<transfers::TransferSendAsset>(),
            Entry::of::<transfers::TransferWithdraw>(),
            Entry::of::<subscribe::SubscribeTrades>(),
            Entry::of::<subscribe::SubscribeOrderbook>(),
            Entry::of::<subscribe::SubscribeCandles>(),
            Entry::of::<subscribe::SubscribeAllMids>(),
            Entry::of::<subscribe::SubscribeOrderUpdates>(),
            Entry::of::<subscribe::SubscribeFills>(),
            Entry::of::<watch_risk::WatchRisk>(),
            Entry::of::<schema::Schema>(),
            Entry::of::<setup::Setup>(),
            Entry::of::<feedback::Feedback>(),
            Entry::of::<update::Update>(),
        ]
    });
    &ENTRIES
}

/// The clap tree of every command, without the global options.
pub fn command_tree() -> clap::Command {
    crate::command_spec::command_tree(entries(), groups())
}

/// The command groups: names with children and no behavior of their own.
pub fn groups() -> &'static [GroupSpec] {
    static GROUPS: &[GroupSpec] = &[
        perps::GROUP,
        spot::GROUP,
        asset::GROUP,
        outcomes::GROUP,
        builder::GROUP,
        account::GROUP,
        api_wallet::GROUP,
        subaccounts::GROUP,
        orders::cli::GROUP,
        watch_risk::GROUP,
        positions::GROUP,
        transfers::GROUP,
        wallet::GROUP,
        staking::GROUP,
        staking::LINK_GROUP,
        vaults::GROUP,
        borrowlend::GROUP,
        prio::GROUP,
        referral::GROUP,
        subscribe::GROUP,
    ];
    GROUPS
}

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
