use serde::Serialize;

pub(crate) trait CatalogCommandMetadata {
    fn aliases(&self) -> &[String];
    fn lifecycle(&self) -> Option<&str>;
    fn risk(&self) -> Option<&str>;
    fn dry_run(&self) -> Option<&str>;
    fn raw_payload(&self) -> Option<&str>;
    fn confirmation(&self) -> Option<&str>;
    fn ows_signer(&self) -> Option<&str>;
    fn auth_required(&self) -> bool;
    fn dangerous(&self) -> bool;
}

pub(crate) trait CatalogArgMetadata {
    fn id(&self) -> &str;
    fn long(&self) -> Option<&str>;
    fn arg_type(&self) -> &str;
    fn description(&self) -> Option<&str>;
    fn input_kind(&self) -> Option<&str>;
    fn set_input_kind(&mut self, input_kind: String);
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CommandMetadata {
    pub(crate) aliases: Vec<String>,
    pub(crate) lifecycle: String,
    pub(crate) risk: String,
    pub(crate) dry_run: String,
    pub(crate) raw_payload: String,
    pub(crate) confirmation: String,
    pub(crate) ows_signer: String,
}

pub(crate) fn command_metadata(
    command: &impl CatalogCommandMetadata,
    command_key: &str,
) -> CommandMetadata {
    CommandMetadata {
        aliases: if command.aliases().is_empty() {
            inferred_aliases(command_key)
        } else {
            command.aliases().to_vec()
        },
        lifecycle: command
            .lifecycle()
            .map(str::to_string)
            .unwrap_or_else(|| inferred_lifecycle(command_key, command)),
        risk: command
            .risk()
            .map(str::to_string)
            .unwrap_or_else(|| inferred_risk(command_key, command)),
        dry_run: command
            .dry_run()
            .map(str::to_string)
            .unwrap_or_else(|| inferred_dry_run(command_key, command)),
        raw_payload: command
            .raw_payload()
            .map(str::to_string)
            .unwrap_or_else(|| inferred_raw_payload(command_key, command)),
        confirmation: command
            .confirmation()
            .map(str::to_string)
            .unwrap_or_else(|| inferred_confirmation(command_key, command)),
        ows_signer: command
            .ows_signer()
            .map(str::to_string)
            .unwrap_or_else(|| inferred_ows_signer(command_key, command)),
    }
}

pub(crate) fn normalize_arg<T: CatalogArgMetadata>(mut arg: T) -> T {
    if arg.input_kind().is_none()
        && let Some(input_kind) = inferred_input_kind(&arg)
    {
        arg.set_input_kind(input_kind);
    }
    arg
}

fn inferred_aliases(command_key: &str) -> Vec<String> {
    if command_key == "transfer" || command_key.starts_with("transfer ") {
        return vec!["transfers".to_string()];
    }
    if command_key == "subaccount" || command_key.starts_with("subaccount ") {
        return vec!["subaccounts".to_string()];
    }
    if command_key == "api-wallet" || command_key.starts_with("api-wallet ") {
        return vec!["api-wallets".to_string()];
    }
    if command_key == "vault" || command_key.starts_with("vault ") {
        return vec!["vaults".to_string()];
    }
    Vec::new()
}

fn inferred_lifecycle(command_key: &str, command: &impl CatalogCommandMetadata) -> String {
    if command_key.starts_with("subscribe ") {
        return "streaming".to_string();
    }
    if is_interactive_local_command(command_key) {
        return "interactive_local".to_string();
    }
    if is_local_state_command(command_key) || is_local_secret_command(command_key) {
        return "read_only".to_string();
    }
    if command.dangerous() {
        return "live_mutating".to_string();
    }
    "read_only".to_string()
}

fn inferred_risk(command_key: &str, command: &impl CatalogCommandMetadata) -> String {
    if is_local_state_command(command_key) {
        return "local_state".to_string();
    }
    if is_local_secret_command(command_key) {
        return "local_secret".to_string();
    }
    if command_key == "account abstraction set" {
        return "account_state".to_string();
    }
    if command.dangerous() {
        return "funds_movement".to_string();
    }
    if command.auth_required() {
        return "account_state".to_string();
    }
    "none".to_string()
}

fn inferred_dry_run(command_key: &str, command: &impl CatalogCommandMetadata) -> String {
    if supports_local_dry_run(command_key)
        || (!is_local_only_command(command_key) && command.dangerous())
    {
        return "optional".to_string();
    }
    "not_supported".to_string()
}

fn inferred_raw_payload(command_key: &str, command: &impl CatalogCommandMetadata) -> String {
    if is_local_only_command(command_key) {
        return "unsupported".to_string();
    }
    match inferred_dry_run(command_key, command).as_str() {
        "optional" | "dry_run_only" => "dry_run_only".to_string(),
        _ => "unsupported".to_string(),
    }
}

fn inferred_confirmation(command_key: &str, command: &impl CatalogCommandMetadata) -> String {
    if requires_local_prompt(command_key) {
        return "prompt".to_string();
    }
    if is_local_only_command(command_key) {
        return "none".to_string();
    }
    if command.dangerous() {
        return "prompt".to_string();
    }
    "none".to_string()
}

fn inferred_ows_signer(command_key: &str, command: &impl CatalogCommandMetadata) -> String {
    if matches!(command_key, "wallet show" | "wallet address") {
        return "address_selector_supported".to_string();
    }
    if command_key == "prio bid" {
        return "experimental_feature_gated".to_string();
    }
    if is_local_only_command(command_key) {
        return "not_applicable".to_string();
    }
    if command.auth_required() {
        return "experimental_feature_gated".to_string();
    }
    "not_required".to_string()
}

fn is_local_only_command(command_key: &str) -> bool {
    is_interactive_local_command(command_key)
        || is_local_state_command(command_key)
        || is_local_secret_command(command_key)
}

fn is_interactive_local_command(command_key: &str) -> bool {
    matches!(
        command_key,
        "setup"
            | "wallet create"
            | "wallet import"
            | "wallet import-mnemonic"
            | "wallet reset"
            | "wallet rename"
            | "wallet delete"
            | "wallet export"
            | "account add"
            | "account set-default"
            | "account remove"
    )
}

fn is_local_state_command(command_key: &str) -> bool {
    matches!(
        command_key,
        "wallet reset"
            | "wallet list"
            | "wallet rename"
            | "wallet delete"
            | "account ls"
            | "account set-default"
            | "account remove"
    )
}

fn is_local_secret_command(command_key: &str) -> bool {
    matches!(
        command_key,
        "setup"
            | "wallet create"
            | "wallet import"
            | "wallet import-mnemonic"
            | "wallet show"
            | "wallet address"
            | "wallet export"
            | "account add"
    )
}

fn supports_local_dry_run(command_key: &str) -> bool {
    matches!(
        command_key,
        "wallet create"
            | "wallet import"
            | "wallet import-mnemonic"
            | "wallet reset"
            | "wallet delete"
            | "account add"
            | "account set-default"
            | "account remove"
    )
}

fn requires_local_prompt(command_key: &str) -> bool {
    matches!(
        command_key,
        "setup"
            | "wallet import"
            | "wallet import-mnemonic"
            | "wallet reset"
            | "wallet delete"
            | "wallet export"
            | "account add"
            | "account set-default"
            | "account remove"
            | "staking link initiate"
            | "staking link finalize"
    )
}

fn inferred_input_kind(arg: &impl CatalogArgMetadata) -> Option<String> {
    let id = arg.id();
    let long = arg.long().unwrap_or_default();
    let description = arg.description().unwrap_or_default().to_ascii_lowercase();

    if description.contains("explicit destination address")
        || description.contains("destination arbitrum address")
    {
        return Some("raw_destination_address".to_string());
    }
    if description.contains("stored account alias")
        || description.contains("stored account id")
        || description.contains("account selector")
    {
        return Some("signer_selector".to_string());
    }
    if description.contains("user address, account alias, or account id")
        || description.contains("ethereum address, account alias, or account id")
        || description.contains("master address, account alias, or account id")
    {
        return Some("public_user_selector".to_string());
    }
    if description.contains("acting-account selector") {
        return Some("acting_account_selector".to_string());
    }
    if description.contains("validator address")
        || description.contains("vault address")
        || description.contains("builder address")
        || description.contains("contract address")
        || description.contains("protocol user address")
    {
        return Some("protocol_object_address".to_string());
    }
    if matches!(id, "coin" | "token" | "pair") {
        return Some("market_symbol".to_string());
    }
    if matches!(id, "source" | "dest" | "dex") {
        return Some("venue_selector".to_string());
    }
    if arg.arg_type() == "boolean" || arg.arg_type() == "bool" {
        return Some("boolean".to_string());
    }
    if matches!(id, "price" | "trigger_price" | "take_profit" | "stop_loss") {
        return Some("price".to_string());
    }
    if matches!(id, "amount" | "size" | "total_size" | "max_fee_rate") {
        return Some("amount".to_string());
    }
    if long.contains("file")
        || id.ends_with("_file")
        || description.contains("local pending artifact path")
    {
        return Some("file_path".to_string());
    }
    if id.contains("duration") || id == "in_duration" || id == "expires_in" {
        return Some("duration".to_string());
    }
    if id == "cloid" || id == "oid" || id.ends_with("_id") || id == "order_id" {
        return Some("identifier".to_string());
    }
    None
}
