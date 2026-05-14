//! Handler binding metadata for the command-spine strangler.
//!
//! These bindings do not route execution yet. They describe which commands are
//! eligible for a future in-process handler and which must remain on legacy
//! fallback because their purity has not been proven.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandlerId {
    Status,
    Meta,
    Book,
    Mids,
    Candles,
    Spread,
    MarketFunding,
    PerpsList,
    PerpsGet,
    SpotList,
    SpotGet,
    OutcomesList,
    OutcomesGet,
    BuilderMaxFee,
    BuilderApproved,
    AccountFills,
    AccountFees,
    AccountRateLimit,
    AccountOrders,
    AccountPortfolio,
    AccountSubaccounts,
    AccountPortfolioHistory,
    AccountLedger,
    AccountFunding,
    AccountTwapHistory,
    AccountTwapFills,
    AccountAbstraction,
    PrioStatus,
    OrdersStatus,
    SubaccountList,
    BorrowlendRates,
    BorrowlendGet,
    BorrowlendUser,
    StakingValidators,
    StakingSummary,
    StakingRewards,
    StakingHistory,
    VaultList,
    VaultSearch,
    VaultGet,
    VaultPositions,
    Schema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandlerDispatch {
    TypedInProcess,
    LegacyFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyFallback {
    CliDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HandlerBinding {
    pub id: Option<HandlerId>,
    pub dispatch: HandlerDispatch,
    pub fallbacks: Vec<LegacyFallback>,
    pub purity: HandlerPurity,
}

impl HandlerBinding {
    pub fn for_command_key(command_key: &str) -> Self {
        match command_key {
            "status" => Self {
                id: Some(HandlerId::Status),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "meta" => Self {
                id: Some(HandlerId::Meta),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "book" => Self {
                id: Some(HandlerId::Book),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "mids" => Self {
                id: Some(HandlerId::Mids),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "candles" => Self {
                id: Some(HandlerId::Candles),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "spread" => Self {
                id: Some(HandlerId::Spread),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "funding" => Self {
                id: Some(HandlerId::MarketFunding),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "perps list" => Self {
                id: Some(HandlerId::PerpsList),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "perps get" => Self {
                id: Some(HandlerId::PerpsGet),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "spot list" => Self {
                id: Some(HandlerId::SpotList),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "spot get" => Self {
                id: Some(HandlerId::SpotGet),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "outcomes list" => Self {
                id: Some(HandlerId::OutcomesList),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "outcomes get" => Self {
                id: Some(HandlerId::OutcomesGet),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "builder max-fee" => Self {
                id: Some(HandlerId::BuilderMaxFee),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "builder approved" => Self {
                id: Some(HandlerId::BuilderApproved),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "account fills" => Self {
                id: Some(HandlerId::AccountFills),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "account fees" => Self {
                id: Some(HandlerId::AccountFees),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "account rate-limit" => Self {
                id: Some(HandlerId::AccountRateLimit),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "account orders" => Self {
                id: Some(HandlerId::AccountOrders),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "account portfolio" => Self {
                id: Some(HandlerId::AccountPortfolio),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "account subaccounts" => Self {
                id: Some(HandlerId::AccountSubaccounts),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "account portfolio-history" => Self {
                id: Some(HandlerId::AccountPortfolioHistory),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "account ledger" => Self {
                id: Some(HandlerId::AccountLedger),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "account funding" => Self {
                id: Some(HandlerId::AccountFunding),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "account twap-history" => Self {
                id: Some(HandlerId::AccountTwapHistory),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "account twap-fills" => Self {
                id: Some(HandlerId::AccountTwapFills),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "account abstraction" => Self {
                id: Some(HandlerId::AccountAbstraction),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "prio status" => Self {
                id: Some(HandlerId::PrioStatus),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "orders status" => Self {
                id: Some(HandlerId::OrdersStatus),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "subaccount list" => Self {
                id: Some(HandlerId::SubaccountList),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "borrowlend rates" => Self {
                id: Some(HandlerId::BorrowlendRates),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "borrowlend get" => Self {
                id: Some(HandlerId::BorrowlendGet),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "borrowlend user" => Self {
                id: Some(HandlerId::BorrowlendUser),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "staking validators" => Self {
                id: Some(HandlerId::StakingValidators),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "staking summary" => Self {
                id: Some(HandlerId::StakingSummary),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "staking rewards" => Self {
                id: Some(HandlerId::StakingRewards),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "staking history" => Self {
                id: Some(HandlerId::StakingHistory),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "vault list" => Self {
                id: Some(HandlerId::VaultList),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "vault search" => Self {
                id: Some(HandlerId::VaultSearch),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "vault get" => Self {
                id: Some(HandlerId::VaultGet),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "vault positions" => Self {
                id: Some(HandlerId::VaultPositions),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            "schema" => Self {
                id: Some(HandlerId::Schema),
                dispatch: HandlerDispatch::TypedInProcess,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::pure(),
            },
            _ => Self {
                id: None,
                dispatch: HandlerDispatch::LegacyFallback,
                fallbacks: legacy_fallbacks(),
                purity: HandlerPurity::conservative_legacy(),
            },
        }
    }

    pub fn is_in_process_safe(&self) -> bool {
        self.dispatch == HandlerDispatch::TypedInProcess
            && self.purity.in_process_violations().is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HandlerPurity {
    pub direct_stdio: bool,
    pub prompts: bool,
    pub process_global_output: bool,
    pub process_global_cli_state: bool,
    pub local_state: bool,
    pub signs_or_submits: bool,
}

impl HandlerPurity {
    pub const fn pure() -> Self {
        Self {
            direct_stdio: false,
            prompts: false,
            process_global_output: false,
            process_global_cli_state: false,
            local_state: false,
            signs_or_submits: false,
        }
    }

    pub const fn conservative_legacy() -> Self {
        Self {
            direct_stdio: true,
            prompts: true,
            process_global_output: true,
            process_global_cli_state: true,
            local_state: true,
            signs_or_submits: true,
        }
    }

    pub fn in_process_violations(&self) -> Vec<&'static str> {
        [
            (self.direct_stdio, "direct_stdio"),
            (self.prompts, "prompts"),
            (self.process_global_output, "process_global_output"),
            (self.process_global_cli_state, "process_global_cli_state"),
            (self.local_state, "local_state"),
            (self.signs_or_submits, "signs_or_submits"),
        ]
        .into_iter()
        .filter_map(|(blocked, name)| blocked.then_some(name))
        .collect()
    }
}

fn legacy_fallbacks() -> Vec<LegacyFallback> {
    vec![LegacyFallback::CliDispatch]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrated_read_only_commands_have_typed_in_process_handler_bindings() {
        for (command_key, expected_id) in [
            ("status", HandlerId::Status),
            ("meta", HandlerId::Meta),
            ("book", HandlerId::Book),
            ("mids", HandlerId::Mids),
            ("candles", HandlerId::Candles),
            ("spread", HandlerId::Spread),
            ("funding", HandlerId::MarketFunding),
            ("perps list", HandlerId::PerpsList),
            ("perps get", HandlerId::PerpsGet),
            ("spot list", HandlerId::SpotList),
            ("spot get", HandlerId::SpotGet),
            ("outcomes list", HandlerId::OutcomesList),
            ("outcomes get", HandlerId::OutcomesGet),
            ("builder max-fee", HandlerId::BuilderMaxFee),
            ("builder approved", HandlerId::BuilderApproved),
            ("account fills", HandlerId::AccountFills),
            ("account fees", HandlerId::AccountFees),
            ("account rate-limit", HandlerId::AccountRateLimit),
            ("account orders", HandlerId::AccountOrders),
            ("account portfolio", HandlerId::AccountPortfolio),
            ("account subaccounts", HandlerId::AccountSubaccounts),
            (
                "account portfolio-history",
                HandlerId::AccountPortfolioHistory,
            ),
            ("account ledger", HandlerId::AccountLedger),
            ("account funding", HandlerId::AccountFunding),
            ("account twap-history", HandlerId::AccountTwapHistory),
            ("account twap-fills", HandlerId::AccountTwapFills),
            ("account abstraction", HandlerId::AccountAbstraction),
            ("prio status", HandlerId::PrioStatus),
            ("orders status", HandlerId::OrdersStatus),
            ("subaccount list", HandlerId::SubaccountList),
            ("borrowlend rates", HandlerId::BorrowlendRates),
            ("borrowlend get", HandlerId::BorrowlendGet),
            ("borrowlend user", HandlerId::BorrowlendUser),
            ("staking validators", HandlerId::StakingValidators),
            ("staking summary", HandlerId::StakingSummary),
            ("staking rewards", HandlerId::StakingRewards),
            ("staking history", HandlerId::StakingHistory),
            ("vault list", HandlerId::VaultList),
            ("vault search", HandlerId::VaultSearch),
            ("vault get", HandlerId::VaultGet),
            ("vault positions", HandlerId::VaultPositions),
            ("schema", HandlerId::Schema),
        ] {
            let binding = HandlerBinding::for_command_key(command_key);

            assert_eq!(binding.id, Some(expected_id), "{command_key}");
            assert_eq!(
                binding.dispatch,
                HandlerDispatch::TypedInProcess,
                "{command_key}"
            );
            assert!(binding.is_in_process_safe(), "{command_key}");
            assert!(binding.fallbacks.contains(&LegacyFallback::CliDispatch));
        }
    }

    #[test]
    fn unmigrated_commands_remain_on_legacy_fallback() {
        let binding = HandlerBinding::for_command_key("orders create");

        assert_eq!(binding.id, None);
        assert_eq!(binding.dispatch, HandlerDispatch::LegacyFallback);
        assert!(!binding.is_in_process_safe());
    }

    #[test]
    fn purity_gate_reports_every_in_process_blocker() {
        let purity = HandlerPurity::conservative_legacy();

        assert_eq!(
            purity.in_process_violations(),
            vec![
                "direct_stdio",
                "prompts",
                "process_global_output",
                "process_global_cli_state",
                "local_state",
                "signs_or_submits",
            ]
        );
    }
}
