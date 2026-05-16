mod contract_support;

use std::collections::{BTreeMap, BTreeSet};

use hyperliquid_cli::command_registry::{
    CommandRegistry, ConfirmationPolicy, DryRunPolicy, Lifecycle, OutputContract, RawPayloadPolicy,
    Risk,
};
use serde_json::{Value, json};

use contract_support::{assert_json_fixture, command_path, schema_all};

#[test]
fn typed_registry_snapshot_matches_characterization_fixture() {
    let registry = CommandRegistry::load().unwrap();

    assert_json_fixture(
        "registry_inventory.json",
        &json!({
            "characterization": true,
            "review_required_to_update": true,
            "commands": registry.entries(),
        }),
    );
}

#[test]
fn registry_inventory_matches_schema_contracts() {
    let registry = CommandRegistry::load().unwrap();
    let schemas = schema_all();
    let schemas_by_path = schemas
        .iter()
        .map(|schema| (command_path(schema), schema))
        .collect::<BTreeMap<_, _>>();
    let registry_paths = registry
        .entries()
        .iter()
        .map(|command| command.command_key())
        .collect::<BTreeSet<_>>();
    let schema_paths = schemas_by_path.keys().cloned().collect::<BTreeSet<_>>();

    assert_eq!(registry_paths, schema_paths);

    for command in registry.entries() {
        let command_key = command.command_key();
        let schema = schemas_by_path
            .get(&command_key)
            .unwrap_or_else(|| panic!("missing schema for {command_key}"));
        let command_value = serde_json::to_value(command).unwrap();

        for field in [
            "command",
            "command_path",
            "aliases",
            "group",
            "description",
            "auth_required",
            "dangerous",
            "lifecycle",
            "risk",
            "dry_run",
            "raw_payload",
            "confirmation",
        ] {
            assert_eq!(
                command_value[field], schema[field],
                "registry/schema drift for {command_key} field {field}"
            );
        }
        assert_eq!(
            command_value["ows_signer"], schema["json_schema"]["x-hyperliquid"]["ows_signer"],
            "registry/schema drift for {command_key} field ows_signer"
        );
        assert_one_of_required_matches_schema(
            &command_key,
            &command_value["one_of_required"],
            &schema["json_schema"]["oneOf"],
        );
        assert_inputs_match_schema(&command_key, &command_value["inputs"], &schema["args"]);
    }
}

#[test]
fn registry_explicitly_classifies_critical_command_classes() {
    let registry = CommandRegistry::load().unwrap();

    let orders_create = registry.find_path(&["orders", "create"]).unwrap();
    assert!(orders_create.dangerous);
    assert!(!orders_create.handler.is_in_process_safe());
    assert_eq!(orders_create.risk, Risk::FundsMovement);
    assert_eq!(orders_create.dry_run, DryRunPolicy::Optional);
    assert_eq!(orders_create.raw_payload, RawPayloadPolicy::DryRunOnly);
    assert_eq!(orders_create.confirmation, ConfirmationPolicy::Prompt);

    let prio_status = registry.find_path(&["prio", "status"]).unwrap();
    assert_eq!(prio_status.risk, Risk::None);
    assert_eq!(prio_status.dry_run, DryRunPolicy::NotSupported);
    assert_eq!(prio_status.raw_payload, RawPayloadPolicy::Unsupported);
    assert_eq!(prio_status.confirmation, ConfirmationPolicy::None);

    let account_abstraction_set = registry
        .find_path(&["account", "abstraction", "set"])
        .unwrap();
    assert_eq!(account_abstraction_set.risk, Risk::AccountState);
    assert_eq!(account_abstraction_set.dry_run, DryRunPolicy::Optional);

    let subscribe = registry.find_path(&["subscribe", "all-mids"]).unwrap();
    assert_eq!(subscribe.lifecycle, Lifecycle::Streaming);
    assert_eq!(
        subscribe.output_contract,
        OutputContract::BoundedNdjsonStream
    );
    assert!(!subscribe.transport.is_empty());

    let status = registry.find_path(&["status"]).unwrap();
    assert!(status.handler.is_in_process_safe());

    let meta = registry.find_path(&["meta"]).unwrap();
    assert!(meta.handler.is_in_process_safe());

    for path in [
        &["book"][..],
        &["mids"][..],
        &["candles"][..],
        &["spread"][..],
        &["funding"][..],
        &["perps", "list"][..],
        &["perps", "get"][..],
        &["spot", "list"][..],
        &["spot", "get"][..],
        &["account", "fills"][..],
        &["account", "fees"][..],
        &["account", "rate-limit"][..],
        &["account", "orders"][..],
        &["account", "portfolio"][..],
        &["account", "subaccounts"][..],
        &["account", "portfolio-history"][..],
        &["account", "ledger"][..],
        &["account", "funding"][..],
        &["account", "twap-history"][..],
        &["account", "twap-fills"][..],
        &["account", "abstraction"][..],
    ] {
        assert!(
            registry
                .find_path(path)
                .unwrap()
                .handler
                .is_in_process_safe()
        );
    }

    for path in [
        &["setup"][..],
        &["wallet", "create"][..],
        &["wallet", "import"][..],
        &["wallet", "reset"][..],
        &["account", "add"][..],
        &["account", "set-default"][..],
        &["account", "remove"][..],
    ] {
        let command = registry.find_path(path).unwrap();
        assert_eq!(command.lifecycle, Lifecycle::InteractiveLocal, "{path:?}");
    }

    for path in [
        &["setup"][..],
        &["wallet", "create"][..],
        &["wallet", "import"][..],
        &["wallet", "show"][..],
        &["wallet", "address"][..],
        &["account", "add"][..],
    ] {
        let command = registry.find_path(path).unwrap();
        assert_eq!(command.risk, Risk::LocalSecret, "{path:?}");
    }

    for path in [
        &["account", "ls"][..],
        &["account", "set-default"][..],
        &["account", "remove"][..],
    ] {
        let command = registry.find_path(path).unwrap();
        assert_eq!(command.risk, Risk::LocalState, "{path:?}");
    }

    let staking_link_initiate = registry
        .find_path(&["staking", "link", "initiate"])
        .unwrap();
    assert_eq!(staking_link_initiate.risk, Risk::FundsMovement);
    assert_eq!(staking_link_initiate.dry_run, DryRunPolicy::Optional);
    assert_eq!(
        staking_link_initiate.confirmation,
        ConfirmationPolicy::Prompt
    );
    assert!(staking_link_initiate.auth_required);

    for path in [
        &["vault", "deposit"][..],
        &["vault", "withdraw"][..],
        &["borrowlend", "supply"][..],
        &["borrowlend", "withdraw"][..],
    ] {
        let command = registry.find_path(path).unwrap();
        assert_eq!(command.risk, Risk::FundsMovement, "{path:?}");
        assert_eq!(command.dry_run, DryRunPolicy::Optional, "{path:?}");
        assert!(command.auth_required, "{path:?}");
    }

    for path in [
        &["builder", "approve"][..],
        &["referral", "set"][..],
        &["referral", "register"][..],
    ] {
        let command = registry.find_path(path).unwrap();
        assert_eq!(command.risk, Risk::FundsMovement, "{path:?}");
        assert_eq!(command.dry_run, DryRunPolicy::Optional, "{path:?}");
        assert!(command.auth_required, "{path:?}");
    }

    for path in [
        &["subaccount", "create"][..],
        &["subaccount", "transfer"][..],
        &["subaccount", "spot-transfer"][..],
    ] {
        let command = registry.find_path(path).unwrap();
        assert_eq!(command.risk, Risk::FundsMovement, "{path:?}");
        assert_eq!(command.dry_run, DryRunPolicy::Optional, "{path:?}");
        assert!(command.auth_required, "{path:?}");
    }

    let subaccount_transfer = registry.find_path(&["subaccount", "transfer"]).unwrap();
    let subaccount_input = subaccount_transfer
        .inputs
        .iter()
        .find(|input| input.id == "subaccount")
        .unwrap();
    assert_eq!(
        subaccount_input.kind.as_ref().map(|kind| kind.as_str()),
        Some("acting_account_selector")
    );

    let builder_approve = registry.find_path(&["builder", "approve"]).unwrap();
    let builder_input = builder_approve
        .inputs
        .iter()
        .find(|input| input.id == "builder")
        .unwrap();
    assert_eq!(
        builder_input.kind.as_ref().map(|kind| kind.as_str()),
        Some("protocol_object_address")
    );
}

#[test]
fn authenticated_reads_do_not_inherit_mutating_preview_policy() {
    let registry = CommandRegistry::load().unwrap();

    for path in [
        &["api-wallet", "list"][..],
        &["orders", "open"][..],
        &["orders", "history"][..],
        &["positions", "list"][..],
        &["referral", "status"][..],
    ] {
        let command = registry.find_path(path).unwrap();
        assert_eq!(command.lifecycle, Lifecycle::ReadOnly, "{path:?}");
        assert_eq!(command.risk, Risk::AccountState, "{path:?}");
        assert_eq!(command.dry_run, DryRunPolicy::NotSupported, "{path:?}");
        assert_eq!(
            command.raw_payload,
            RawPayloadPolicy::Unsupported,
            "{path:?}"
        );
    }

    for path in [
        &["subscribe", "order-updates"][..],
        &["subscribe", "fills"][..],
    ] {
        let command = registry.find_path(path).unwrap();
        assert_eq!(command.lifecycle, Lifecycle::Streaming, "{path:?}");
        assert_eq!(command.risk, Risk::AccountState, "{path:?}");
        assert_eq!(command.dry_run, DryRunPolicy::NotSupported, "{path:?}");
        assert_eq!(
            command.raw_payload,
            RawPayloadPolicy::Unsupported,
            "{path:?}"
        );
    }
}

fn assert_inputs_match_schema(command_key: &str, inputs: &Value, args: &Value) {
    let inputs = inputs
        .as_array()
        .unwrap_or_else(|| panic!("registry inputs missing for {command_key}"));
    let args = args
        .as_array()
        .unwrap_or_else(|| panic!("schema args missing for {command_key}"));
    let args_by_id = args
        .iter()
        .map(|arg| {
            (
                arg["id"]
                    .as_str()
                    .unwrap_or_else(|| panic!("schema arg missing id for {command_key}")),
                arg,
            )
        })
        .collect::<BTreeMap<_, _>>();

    assert_eq!(
        inputs.len(),
        args.len(),
        "registry/schema input count drift for {command_key}"
    );

    for input in inputs {
        let input_id = input["id"]
            .as_str()
            .unwrap_or_else(|| panic!("registry input missing id for {command_key}"));
        let arg = args_by_id.get(input_id).unwrap_or_else(|| {
            panic!("registry input {input_id} missing from schema for {command_key}")
        });
        for field in [
            "long",
            "positional_index",
            "arg_type",
            "required",
            "multiple",
            "enum_values",
            "description",
            "default",
        ] {
            assert_eq!(
                input[field], arg[field],
                "registry/schema input drift for {command_key} arg {input_id} field {field}"
            );
        }
        assert_eq!(
            input["kind"], arg["input_kind"],
            "registry/schema input_kind drift for {command_key} arg {input_id}"
        );
    }
}

fn assert_one_of_required_matches_schema(command_key: &str, registry: &Value, schema: &Value) {
    let expected = registry
        .as_array()
        .unwrap_or_else(|| panic!("registry one_of_required missing for {command_key}"))
        .iter()
        .map(|required| json!({ "required": required }))
        .collect::<Vec<_>>();
    let actual = schema.as_array().cloned().unwrap_or_default();

    assert_eq!(
        actual, expected,
        "registry/schema oneOf drift for {command_key}"
    );
}
