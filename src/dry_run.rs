//! Stable dry-run envelope and narrow action-plan spike.
//!
//! Dry-run output remains a public CLI contract. `ActionPlan` is intentionally
//! small and internal-facing for now; command-family migrations can consume it
//! later without forcing every signed action through one generic abstraction.

use serde::Serialize;

use crate::network::Network;
use crate::output::TableData;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    SignedExchangeAction,
    LocalStateMutation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionReversibility {
    Reversible,
    PartiallyReversible,
    Irreversible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveSubmissionPolicy {
    DryRunOnly,
    ValidateConfirmSignSubmit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionPlan {
    would_execute: String,
    kind: ActionKind,
    reversibility: ActionReversibility,
    live_submission: LiveSubmissionPolicy,
}

impl ActionPlan {
    #[must_use]
    pub fn signed_exchange_action(
        would_execute: impl Into<String>,
        reversibility: ActionReversibility,
        live_submission: LiveSubmissionPolicy,
    ) -> Self {
        Self {
            would_execute: would_execute.into(),
            kind: ActionKind::SignedExchangeAction,
            reversibility,
            live_submission,
        }
    }

    #[must_use]
    pub fn would_execute(&self) -> &str {
        &self.would_execute
    }

    #[must_use]
    pub fn kind(&self) -> ActionKind {
        self.kind
    }

    #[must_use]
    pub fn reversibility(&self) -> ActionReversibility {
        self.reversibility
    }

    #[must_use]
    pub fn live_submission(&self) -> LiveSubmissionPolicy {
        self.live_submission
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DryRunSigningContext {
    signer: Option<String>,
    acting_as: Option<String>,
    vault_address: Option<String>,
}

impl DryRunSigningContext {
    #[must_use]
    pub fn new(
        signer: Option<String>,
        acting_as: Option<String>,
        vault_address: Option<String>,
    ) -> Self {
        Self {
            signer,
            acting_as,
            vault_address,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DryRunEnvelope {
    command: String,
    details: serde_json::Value,
    run_context: Option<DryRunRunContext>,
}

impl DryRunEnvelope {
    #[must_use]
    pub fn from_details(command: impl Into<String>, details: serde_json::Value) -> Self {
        Self {
            command: command.into(),
            details,
            run_context: None,
        }
    }

    #[must_use]
    pub fn from_action_plan(
        command: impl Into<String>,
        plan: &ActionPlan,
        args: serde_json::Value,
    ) -> Self {
        Self::from_details(
            command,
            serde_json::json!({
                "would_execute": plan.would_execute(),
                "args": args,
            }),
        )
    }

    #[must_use]
    pub fn with_payload(mut self, payload: Option<serde_json::Value>) -> Self {
        if let Some(payload) = payload {
            self.details_object_mut()
                .insert("payload".to_string(), payload);
        }
        self
    }

    #[must_use]
    pub fn with_signing_context(mut self, context: DryRunSigningContext) -> Self {
        let details = self.details_object_mut();
        details.insert("signer".to_string(), serde_json::json!(context.signer));
        details.insert(
            "acting_as".to_string(),
            serde_json::json!(context.acting_as),
        );
        details.insert(
            "vault_address".to_string(),
            serde_json::json!(context.vault_address),
        );
        self
    }

    fn details_object_mut(&mut self) -> &mut serde_json::Map<String, serde_json::Value> {
        if !self.details.is_object() {
            self.details = serde_json::Value::Object(serde_json::Map::new());
        }
        self.details.as_object_mut().expect("details object")
    }
}

/// Network and signer facts for this run, added to a dry-run envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunRunContext {
    network: Network,
    signer: Option<Result<String, String>>,
}

impl DryRunRunContext {
    /// `signer` is `None` for commands that don't sign. For signing commands pass the
    /// resolved address, or the reason it couldn't be resolved without prompting.
    #[must_use]
    pub fn new(network: Network, signer: Option<Result<String, String>>) -> Self {
        Self { network, signer }
    }
}

impl DryRunEnvelope {
    /// Add the run's network and signer to what this envelope prints.
    #[must_use]
    pub fn with_run_context(mut self, context: Option<DryRunRunContext>) -> Self {
        self.run_context = context;
        self
    }

    fn merged_object(
        &self,
        context: Option<&DryRunRunContext>,
    ) -> serde_json::Map<String, serde_json::Value> {
        let mut object = serde_json::Map::new();
        object.insert("dry_run".to_string(), serde_json::json!(true));
        object.insert("command".to_string(), serde_json::json!(self.command));
        if let Some(details) = self.details.as_object() {
            for (key, value) in details {
                object.insert(key.clone(), value.clone());
            }
        }
        let Some(context) = context else {
            return object;
        };
        object.insert(
            "network".to_string(),
            serde_json::json!(context.network.to_string()),
        );
        let signer_known = object.get("signer").is_some_and(|value| !value.is_null());
        match (&context.signer, signer_known) {
            (Some(Ok(address)), false) => {
                object.insert("signer".to_string(), serde_json::json!(address));
            }
            (Some(Err(reason)), false) => {
                object.insert("signer".to_string(), serde_json::Value::Null);
                object.insert("signer_warning".to_string(), serde_json::json!(reason));
            }
            _ => {}
        }
        object
    }

    fn rows_for(&self, context: Option<&DryRunRunContext>) -> Vec<Vec<String>> {
        let object = self.merged_object(context);
        let text = |key: &str| object.get(key).and_then(serde_json::Value::as_str);
        let mut rows = vec![
            vec!["Command".to_string(), self.command.clone()],
            vec![
                "Would execute".to_string(),
                text("would_execute")
                    .unwrap_or("validate_and_execute")
                    .to_string(),
            ],
        ];
        match object.get("args") {
            Some(serde_json::Value::Object(args)) => {
                for (key, value) in args {
                    rows.push(vec![field_label(key), display_value(value)]);
                }
            }
            Some(serde_json::Value::Null) | None => {}
            Some(other) => rows.push(vec!["Args".to_string(), display_value(other)]),
        }
        for (key, label) in [
            ("signer", "Signer"),
            ("acting_as", "Acting as"),
            ("vault_address", "Vault"),
        ] {
            if let Some(value) = object.get(key)
                && !value.is_null()
            {
                rows.push(vec![label.to_string(), display_value(value)]);
            } else if key == "signer" && object.contains_key("signer_warning") {
                rows.push(vec!["Signer".to_string(), "not resolved".to_string()]);
            }
        }
        if let Some(warning) = text("signer_warning") {
            rows.push(vec!["Warning".to_string(), warning.to_string()]);
        }
        if let Some(network) = text("network") {
            rows.push(vec!["Network".to_string(), network.to_string()]);
        }
        rows
    }
}

fn field_label(key: &str) -> String {
    let spaced = key.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn display_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Null => "none".to_string(),
        other => other.to_string(),
    }
}

impl TableData for DryRunEnvelope {
    fn headers(&self) -> Vec<&str> {
        vec!["Field", "Value"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows_for(self.run_context.as_ref())
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::Value::Object(self.merged_object(self.run_context.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_plan_records_precise_live_submission_terms() {
        let plan = ActionPlan::signed_exchange_action(
            "create_subaccount",
            ActionReversibility::Irreversible,
            LiveSubmissionPolicy::ValidateConfirmSignSubmit,
        );

        assert_eq!(plan.would_execute(), "create_subaccount");
        assert_eq!(plan.kind(), ActionKind::SignedExchangeAction);
        assert_eq!(plan.reversibility(), ActionReversibility::Irreversible);
        assert_eq!(
            plan.live_submission(),
            LiveSubmissionPolicy::ValidateConfirmSignSubmit
        );
    }

    #[test]
    fn action_plan_envelope_preserves_current_public_json_shape() {
        let plan = ActionPlan::signed_exchange_action(
            "create_subaccount",
            ActionReversibility::Irreversible,
            LiveSubmissionPolicy::ValidateConfirmSignSubmit,
        );
        let envelope = DryRunEnvelope::from_action_plan(
            "subaccount create",
            &plan,
            serde_json::json!({"name": "market-maker-1"}),
        )
        .with_signing_context(DryRunSigningContext::new(None, None, None));

        assert_eq!(
            envelope.to_json_value(),
            serde_json::json!({
                "dry_run": true,
                "command": "subaccount create",
                "would_execute": "create_subaccount",
                "args": {"name": "market-maker-1"},
                "signer": null,
                "acting_as": null,
                "vault_address": null
            })
        );
    }

    fn sample_envelope() -> DryRunEnvelope {
        let plan = ActionPlan::signed_exchange_action(
            "place_order",
            ActionReversibility::PartiallyReversible,
            LiveSubmissionPolicy::ValidateConfirmSignSubmit,
        );
        DryRunEnvelope::from_action_plan(
            "orders create",
            &plan,
            serde_json::json!({"coin": "BTC", "side": "buy", "size": "0.1", "price": null}),
        )
    }

    #[test]
    fn run_context_adds_network_and_resolved_signer() {
        let context = DryRunRunContext {
            network: Network::Testnet,
            signer: Some(Ok("0xabc".to_string())),
        };
        let json = serde_json::Value::Object(sample_envelope().merged_object(Some(&context)));

        assert_eq!(json["network"], "testnet");
        assert_eq!(json["signer"], "0xabc");
        assert!(json.get("signer_warning").is_none());
    }

    #[test]
    fn unresolved_signer_is_null_with_warning() {
        let context = DryRunRunContext {
            network: Network::Mainnet,
            signer: Some(Err("no signer configured".to_string())),
        };
        let json = serde_json::Value::Object(sample_envelope().merged_object(Some(&context)));

        assert_eq!(json["network"], "mainnet");
        assert!(json["signer"].is_null());
        assert_eq!(json["signer_warning"], "no signer configured");
    }

    #[test]
    fn existing_signer_is_kept_and_unsigned_commands_get_no_signer_keys() {
        let envelope = sample_envelope().with_signing_context(DryRunSigningContext::new(
            Some("0xexisting".to_string()),
            None,
            None,
        ));
        let signed = DryRunRunContext {
            network: Network::Mainnet,
            signer: Some(Ok("0xother".to_string())),
        };
        let json = serde_json::Value::Object(envelope.merged_object(Some(&signed)));
        assert_eq!(json["signer"], "0xexisting");

        let unsigned = DryRunRunContext {
            network: Network::Mainnet,
            signer: None,
        };
        let json = serde_json::Value::Object(sample_envelope().merged_object(Some(&unsigned)));
        assert_eq!(json["network"], "mainnet");
        assert!(json.get("signer").is_none());
    }

    #[test]
    fn human_rows_list_args_signer_and_network() {
        let context = DryRunRunContext {
            network: Network::Testnet,
            signer: Some(Ok("0xabc".to_string())),
        };
        let rows = sample_envelope().rows_for(Some(&context));
        let find = |label: &str| {
            rows.iter()
                .find(|row| row[0] == label)
                .map(|row| row[1].clone())
        };

        assert_eq!(find("Command").as_deref(), Some("orders create"));
        assert_eq!(find("Would execute").as_deref(), Some("place_order"));
        assert_eq!(find("Coin").as_deref(), Some("BTC"));
        assert_eq!(find("Side").as_deref(), Some("buy"));
        assert_eq!(find("Size").as_deref(), Some("0.1"));
        assert_eq!(find("Price").as_deref(), Some("none"));
        assert_eq!(find("Signer").as_deref(), Some("0xabc"));
        assert_eq!(find("Network").as_deref(), Some("testnet"));
    }
}
