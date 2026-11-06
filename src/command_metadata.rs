use serde::Serialize;

pub(crate) trait CatalogCommandMetadata {
    fn aliases(&self) -> &[String];
    fn lifecycle(&self) -> &str;
    fn risk(&self) -> &str;
    fn dry_run(&self) -> &str;
    fn raw_payload(&self) -> &str;
    fn confirmation(&self) -> &str;
    fn ows_signer(&self) -> &str;
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
        lifecycle: command.lifecycle().to_string(),
        risk: command.risk().to_string(),
        dry_run: command.dry_run().to_string(),
        raw_payload: command.raw_payload().to_string(),
        confirmation: command.confirmation().to_string(),
        ows_signer: command.ows_signer().to_string(),
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
