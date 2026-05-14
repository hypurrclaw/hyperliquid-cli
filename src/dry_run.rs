//! Stable dry-run envelope and narrow action-plan spike.
//!
//! Dry-run output remains a public CLI contract. `ActionPlan` is intentionally
//! small and internal-facing for now; command-family migrations can consume it
//! later without forcing every signed action through one generic abstraction.

use serde::Serialize;

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
}

impl DryRunEnvelope {
    #[must_use]
    pub fn from_details(command: impl Into<String>, details: serde_json::Value) -> Self {
        Self {
            command: command.into(),
            details,
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

impl TableData for DryRunEnvelope {
    fn headers(&self) -> Vec<&str> {
        vec!["Command", "Dry Run", "Would Execute"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.command.clone(),
            "true".to_string(),
            self.details
                .get("would_execute")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("validate_and_execute")
                .to_string(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        let mut object = serde_json::Map::new();
        object.insert("dry_run".to_string(), serde_json::json!(true));
        object.insert("command".to_string(), serde_json::json!(self.command));
        if let Some(details) = self.details.as_object() {
            for (key, value) in details {
                object.insert(key.clone(), value.clone());
            }
        }
        serde_json::Value::Object(object)
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
}
