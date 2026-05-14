//! Machine-readable command schema output for agents.

use serde::Serialize;
use serde_json::{Value, json};

use crate::command_registry::{CommandContract, CommandRegistry, InputContract, InputKind};
use crate::errors::CliError;
use crate::output::{self, OutputFormat, TableData};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CatalogArg {
    id: String,
    long: Option<String>,
    short: Option<String>,
    positional_index: Option<usize>,
    arg_type: String,
    required: bool,
    multiple: bool,
    enum_values: Vec<String>,
    description: Option<String>,
    default: Option<String>,
    input_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CommandSchema {
    command: String,
    command_path: Vec<String>,
    group: String,
    auth_required: bool,
    dangerous: bool,
    aliases: Vec<String>,
    lifecycle: String,
    risk: String,
    mutability: String,
    dry_run: String,
    raw_payload: String,
    confirmation: String,
    confirmation_bypass: Value,
    ows_signer: String,
    transport: Value,
    output_contract: String,
    stream_bounds: Value,
    description: String,
    args: Vec<CatalogArg>,
    json_schema: Value,
}

#[derive(Debug, Clone)]
pub struct SchemaOutput {
    value: Value,
}

impl TableData for SchemaOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Command", "Group", "Dangerous", "Description"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        let commands: Vec<Value> = match &self.value {
            Value::Array(values) => values.clone(),
            value => vec![value.clone()],
        };
        commands
            .iter()
            .map(|command| {
                vec![
                    command
                        .get("command")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    command
                        .get("group")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    command
                        .get("dangerous")
                        .and_then(Value::as_bool)
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    command
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> Value {
        self.value.clone()
    }
}

pub fn show(command_path: &[String], format: OutputFormat) -> Result<(), anyhow::Error> {
    let output = schema_query(command_path)?;
    output::print_data_no_timing(&output, format);
    Ok(())
}

pub fn schema_query(command_path: &[String]) -> Result<SchemaOutput, anyhow::Error> {
    let registry = CommandRegistry::load()
        .map_err(|err| CliError::Internal(anyhow::anyhow!("invalid command registry: {err}")))?;
    let schemas = registry
        .entries()
        .iter()
        .map(command_schema)
        .collect::<Vec<_>>();

    let value = if command_path.is_empty() {
        serde_json::to_value(schemas).map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?
    } else {
        let requested = command_path.join(" ");
        if let Some(schema) = schemas
            .iter()
            .find(|schema| schema.command_path.as_slice() == command_path)
        {
            serde_json::to_value(schema).map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?
        } else {
            let matches = schemas
                .into_iter()
                .filter(|schema| command_path_is_prefix(command_path, &schema.command_path))
                .collect::<Vec<_>>();
            if matches.is_empty() {
                return Err(CliError::Unsupported(format!(
                    "schema command '{requested}' not found"
                ))
                .into());
            }
            serde_json::to_value(matches).map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?
        }
    };

    Ok(SchemaOutput { value })
}

fn command_path_is_prefix(prefix: &[String], command_path: &[String]) -> bool {
    prefix.len() < command_path.len()
        && command_path
            .iter()
            .zip(prefix)
            .all(|(actual, requested)| actual == requested)
}

fn command_schema(command: &CommandContract) -> CommandSchema {
    let metadata = command.metadata_json();
    let args = command
        .inputs
        .iter()
        .map(CatalogArg::from)
        .collect::<Vec<_>>();
    let json_schema = args_to_schema(&args, &command.one_of_required, metadata.clone());
    CommandSchema {
        command: command.command.clone(),
        command_path: command.command_path.clone(),
        group: command.group.clone(),
        auth_required: command.auth_required,
        dangerous: command.dangerous,
        aliases: command.aliases.clone(),
        lifecycle: metadata_string(&metadata, "lifecycle"),
        risk: metadata_string(&metadata, "risk"),
        mutability: metadata_string(&metadata, "mutability"),
        dry_run: metadata_string(&metadata, "dry_run"),
        raw_payload: metadata_string(&metadata, "raw_payload"),
        confirmation: metadata_string(&metadata, "confirmation"),
        confirmation_bypass: metadata
            .get("confirmation_bypass")
            .cloned()
            .unwrap_or_else(|| json!({"supported": false, "arg": null})),
        ows_signer: metadata_string(&metadata, "ows_signer"),
        transport: metadata
            .get("transport")
            .cloned()
            .unwrap_or_else(|| json!([])),
        output_contract: metadata_string(&metadata, "output_contract"),
        stream_bounds: metadata
            .get("stream_bounds")
            .cloned()
            .unwrap_or(Value::Null),
        description: command.description.clone(),
        args,
        json_schema,
    }
}

impl From<&InputContract> for CatalogArg {
    fn from(input: &InputContract) -> Self {
        Self {
            id: input.id.clone(),
            long: input.long.clone(),
            short: input.short.clone(),
            positional_index: input.positional_index,
            arg_type: input.arg_type.clone(),
            required: input.required,
            multiple: input.multiple,
            enum_values: input.enum_values.clone(),
            description: input.description.clone(),
            default: input.default.clone(),
            input_kind: input.kind.map(input_kind_string),
        }
    }
}

fn args_to_schema(args: &[CatalogArg], one_of_required: &[Vec<String>], metadata: Value) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    for arg in args {
        let mut prop = serde_json::Map::new();
        if arg.multiple {
            prop.insert("type".into(), json!("array"));
            prop.insert("items".into(), json!({"type": scalar_type(&arg.arg_type)}));
        } else {
            prop.insert("type".into(), json!(scalar_type(&arg.arg_type)));
        }
        if !arg.enum_values.is_empty() {
            prop.insert(
                "enum".into(),
                Value::Array(arg.enum_values.iter().cloned().map(Value::String).collect()),
            );
        }
        if let Some(description) = &arg.description {
            prop.insert("description".into(), Value::String(description.clone()));
        }
        if let Some(default) = &arg.default {
            prop.insert("default".into(), Value::String(default.clone()));
        }
        if let Some(input_kind) = &arg.input_kind {
            prop.insert("input_kind".into(), Value::String(input_kind.clone()));
        }
        properties.insert(arg.id.clone(), Value::Object(prop));
        if arg.required {
            required.push(Value::String(arg.id.clone()));
        }
    }

    let mut schema = serde_json::Map::new();
    schema.insert("type".into(), json!("object"));
    schema.insert("properties".into(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert("required".into(), Value::Array(required));
    }
    if !one_of_required.is_empty() {
        schema.insert(
            "oneOf".into(),
            Value::Array(
                one_of_required
                    .iter()
                    .map(|required| json!({ "required": required }))
                    .collect(),
            ),
        );
    }
    schema.insert("x-hyperliquid".into(), metadata);
    schema.insert("additionalProperties".into(), json!(false));
    Value::Object(schema)
}

fn input_kind_string(kind: InputKind) -> String {
    kind.as_str().to_string()
}

fn metadata_string(metadata: &Value, field: &str) -> String {
    metadata[field].as_str().unwrap_or_default().to_string()
}

fn scalar_type(arg_type: &str) -> &'static str {
    match arg_type {
        "boolean" | "bool" => "boolean",
        "integer" | "number" => "number",
        _ => "string",
    }
}
