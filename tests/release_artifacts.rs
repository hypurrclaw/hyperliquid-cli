use assert_cmd::Command;
use hyperliquid_cli::command_registry::{CommandRegistry, OwsSupport, PHASE1_AUTHORITY_DECISION};
use predicates::prelude::*;
use std::fs;
use std::path::Path;

fn repo_file(path: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(root.join(path)).unwrap_or_else(|err| panic!("failed to read {path}: {err}"))
}

fn optional_repo_file(path: &str) -> Option<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(root.join(path)).ok()
}

#[test]
fn readme_contains_required_release_sections() {
    let readme = repo_file("README.md");

    for section in [
        "# Hyperliquid CLI",
        "## Install",
        "## Quick start",
        "## Terminology and address selectors",
        "## Command reference",
        "## Configuration",
        "## Testnet",
        "## Output formats",
    ] {
        assert!(
            readme.contains(section),
            "README.md is missing required section {section}"
        );
    }
}

#[test]
fn oss_release_health_files_are_present() {
    for path in [
        "LICENSE",
        "CONTRIBUTING.md",
        ".github/PULL_REQUEST_TEMPLATE.md",
        "scripts/pre-release-check.sh",
    ] {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(
            root.join(path).exists(),
            "missing OSS release health file {path}"
        );
    }
}

#[test]
fn release_install_path_uses_public_repo_and_checksums() {
    let readme = repo_file("README.md");
    let install_script = repo_file("install.sh");
    let release_workflow = repo_file(".github/workflows/release.yml");
    let update_check = repo_file("src/update_check.rs");

    for required in [
        "hypurrclaw/hyperliquid-cli",
        "sh install.sh",
        "cargo install --path . --bin hyperliquid",
    ] {
        assert!(readme.contains(required), "README.md is missing {required}");
    }

    for required in [
        "hypurrclaw/hyperliquid-cli",
        "sha256_check",
        "${asset}.sha256",
    ] {
        assert!(
            install_script.contains(required),
            "install.sh is missing {required}"
        );
    }

    for required in [
        "push:",
        "tags:",
        "hyperliquid-linux-x86_64.tar.gz",
        "hyperliquid-linux-aarch64.tar.gz",
        "hyperliquid-macos-x86_64.tar.gz",
        "hyperliquid-macos-aarch64.tar.gz",
        "hyperliquid-windows-x86_64.zip",
        "sha256sum",
        "softprops/action-gh-release",
    ] {
        assert!(
            release_workflow.contains(required),
            ".github/workflows/release.yml is missing {required}"
        );
    }

    for required in [
        "hyperliquid-linux-x86_64.tar.gz",
        "hyperliquid-linux-aarch64.tar.gz",
        "hyperliquid-macos-x86_64.tar.gz",
        "hyperliquid-macos-aarch64.tar.gz",
    ] {
        assert!(
            update_check.contains(required),
            "src/update_check.rs is missing supported self-update asset mapping {required}"
        );
    }
    assert!(
        update_check.contains("update is not supported on {os}/{arch}"),
        "src/update_check.rs should fail closed for unsupported self-update platforms such as Windows"
    );
}

#[test]
fn public_docs_do_not_reference_local_user_paths() {
    for path in ["README.md", "AGENTS.md"] {
        let text = repo_file(path);
        assert!(
            !text.contains("/Users/studio"),
            "{path} should not contain local workstation paths"
        );
    }
}

#[test]
fn main_entrypoint_stays_thin_and_registry_path_drift_free() {
    let main_rs = repo_file("src/main.rs");
    let runtime_rs = repo_file("src/cli_runtime.rs");
    let main_line_count = main_rs.lines().count();

    assert!(
        main_line_count <= 1_500,
        "src/main.rs should stay a thin parser/bootstrap entrypoint; got {main_line_count} lines"
    );
    assert!(
        main_rs.contains("mod cli_runtime;"),
        "runtime dispatch should stay outside src/main.rs"
    );
    assert!(
        !main_rs.contains("fn command_registry_path(")
            && !runtime_rs.contains("fn command_registry_path("),
        "CLI registry paths should come from clap ArgMatches, not a manual enum match table"
    );
    assert!(
        runtime_rs.contains("command_contract_for_path")
            && runtime_rs.contains("CommandRegistry::load()"),
        "runtime policy should continue resolving command contracts through the registry"
    );
}

#[test]
fn docs_define_agent_safe_terminology() {
    let readme = repo_file("README.md");
    let skills = optional_repo_file("SKILLS.md");
    let agents = repo_file("AGENTS.md");

    for required in [
        "Local signing account",
        "Selected signer",
        "Protocol user address",
        "API wallet / agent wallet",
        "Protocol address",
        "api-wallets` -> `api-wallet`",
        "subaccounts` -> `subaccount`",
        "transfers` -> `transfer`",
        "vaults` -> `vault`",
        "ACCOUNT_SELECTOR",
        "USER",
        "*_ADDRESS",
    ] {
        assert!(
            readme.contains(required),
            "README.md is missing terminology marker {required}"
        );
    }

    if let Some(skills) = skills {
        for required in [
            "Local signing account",
            "Selected signer",
            "API wallet / agent wallet",
            "api-wallets` -> `api-wallet`",
            "subaccounts` -> `subaccount`",
            "transfers` -> `transfer`",
            "vaults` -> `vault`",
            "ACCOUNT_SELECTOR",
            "USER",
            "*_ADDRESS",
        ] {
            assert!(
                skills.contains(required),
                "SKILLS.md is missing terminology marker {required}"
            );
        }
    }

    for required in [
        "local signing account",
        "selected signer",
        "API wallet",
        "ACCOUNT_SELECTOR",
        "*_ADDRESS",
        "schema",
    ] {
        assert!(
            agents.contains(required),
            "AGENTS.md is missing terminology marker {required}"
        );
    }
}

#[test]
fn ci_workflow_runs_release_quality_gates() {
    let workflow = repo_file(".github/workflows/ci.yml");

    for required in [
        "pull_request",
        "push",
        "cargo build",
        "cargo test",
        "Contract characterization tests",
        "cargo test --test command_contracts --test schema_contracts --test registry_contracts --test dry_run_contracts --test output_contracts",
        "cargo clippy -- -D warnings",
        "cargo test --lib --bins",
        "cargo clippy --lib --bins -- -D warnings",
        "cargo fmt --check",
    ] {
        assert!(
            workflow.contains(required),
            ".github/workflows/ci.yml is missing {required}"
        );
    }
}

#[test]
fn taskfile_exposes_contract_characterization_gate() {
    let taskfile = repo_file("Taskfile.yml");
    let fixtures_readme = repo_file("tests/fixtures/contracts/README.md");

    for required in [
        "contracts:",
        "cargo test --test command_contracts --test schema_contracts --test registry_contracts --test dry_run_contracts --test output_contracts",
        "task: contracts",
    ] {
        assert!(
            taskfile.contains(required),
            "Taskfile.yml is missing contract gate marker {required}"
        );
    }

    for required in [
        "characterization snapshots",
        "HYPERLIQUID_UPDATE_CONTRACTS=1",
        "Review the resulting diff",
    ] {
        assert!(
            fixtures_readme.contains(required),
            "contract fixture README is missing {required}"
        );
    }
}

#[test]
fn phase1_registry_authority_is_documented_and_used_for_schema_emitters() {
    assert!(PHASE1_AUTHORITY_DECISION.contains("src/command_catalog.json"));
    assert!(PHASE1_AUTHORITY_DECISION.contains("CommandRegistry"));
    assert!(PHASE1_AUTHORITY_DECISION.contains("CLI schema"));
    assert!(PHASE1_AUTHORITY_DECISION.contains("CLI schemas"));

    let cli_schema = repo_file("src/commands/schema.rs");

    assert!(cli_schema.contains("CommandRegistry::load()"));
    assert!(
        !cli_schema.contains("command_metadata("),
        "CLI schema should emit from CommandRegistry instead of re-inferring metadata"
    );
}

#[test]
fn registry_rollout_policy_and_canary_entry_points_are_release_gated() {
    let Some(policy) = optional_repo_file("docs/registry-rollout-policy.md") else {
        return;
    };
    let taskfile = repo_file("Taskfile.yml");
    let workflow = repo_file(".github/workflows/ci.yml");
    let qa_matrix = optional_repo_file("QA_COMMAND_MATRIX.md");
    let rollout_script = repo_file("scripts/qa-registry-rollout-gates.sh");

    for required in [
        "hidden/internal registry",
        "read-only default",
        "testnet mutating canary",
        "mainnet dry-run comparison",
        "mainnet opt-in",
        "mainnet default",
        "legacy removal",
        "legacy-child",
        "legacy-dispatch",
        "fail-closed",
        ".qa/registry-rollout-canary-",
        "cleanup-orders-open.json",
        "cleanup-positions-list.json",
        "cleanup-account-portfolio.json",
        "Live Order Create And Cancel (#60)",
        "Remaining Live Order Paths (#61)",
        "Funded-live canaries are manual or scheduled QA only",
    ] {
        assert!(
            policy.contains(required),
            "registry rollout policy is missing {required}"
        );
        assert!(
            rollout_script.contains(required),
            "rollout gate script is missing marker {required}"
        );
    }

    for required in ["qa:registry-rollout", "qa:registry-canary-plan"] {
        assert!(
            taskfile.contains(required),
            "Taskfile.yml is missing rollout task {required}"
        );
    }

    assert!(workflow.contains("Registry rollout policy gate"));
    if let Some(qa_matrix) = qa_matrix {
        assert!(qa_matrix.contains("Registry Rollout And Canary Gates"));
        assert!(qa_matrix.contains("task qa:registry-canary-plan"));
    }
}

#[test]
fn registry_authoring_guide_documents_operating_model() {
    let Some(guide) = optional_repo_file("docs/command-registry-authoring.md") else {
        return;
    };
    let readme = repo_file("README.md");

    for required in [
        "Authoring Rules",
        "Selector Semantics",
        "Signer Capabilities",
        "Dry-Run And Raw Payload Policy",
        "Generated Artifact Workflow",
        "src/command_catalog.json",
        "CommandRegistry::load()",
        "input_kind",
        "ACCOUNT_SELECTOR",
        "*_ADDRESS",
        "local_signing_only",
        "dry_run = optional",
        "Raw payload input is `dry_run_only`",
        "--allow-dangerous",
        "HYPERLIQUID_UPDATE_CONTRACTS=1",
    ] {
        assert!(
            guide.contains(required),
            "command registry authoring guide is missing {required}"
        );
    }

    assert!(
        readme.contains("docs/command-registry-authoring.md"),
        "README.md should link the command registry authoring guide"
    );
}

#[test]
fn secret_storage_compatibility_policy_documents_no_side_effect_reads() {
    let Some(policy) = optional_repo_file("docs/secret-storage-compatibility.md") else {
        return;
    };
    let readme = repo_file("README.md");

    for required in [
        "Read-Only No-Side-Effect Gate",
        "schema generation",
        "schema generation",
        "public account lookup",
        "must not migrate, delete, chmod, back up, or rewrite",
        "project missing metadata columns as `null`",
        "must not create an encryption key",
        "Writable Migration Boundary",
        "versioned encrypted-account envelope",
        "compatibility tests for v1 read paths",
        "no plaintext secret emission",
    ] {
        assert!(
            policy.contains(required),
            "secret storage compatibility policy is missing {required}"
        );
    }

    assert!(
        readme.contains("docs/secret-storage-compatibility.md"),
        "README.md should link the secret storage compatibility policy"
    );
}

#[test]
fn qa_matrix_asserts_json_contracts_and_cleanup_checks() {
    let qa_matrix = repo_file("scripts/qa-command-matrix.sh");

    for required in [
        "validate_case_contract",
        "stdout is not valid JSON",
        "expected top-level JSON field",
        "failing JSON command must return an error envelope",
        "cleanup orders open after dry-runs",
        "cleanup positions list after dry-runs",
    ] {
        assert!(
            qa_matrix.contains(required),
            "QA matrix is missing contract assertion marker {required}"
        );
    }
}

#[test]
fn install_script_targets_github_release_binary() {
    let install_script = repo_file("install.sh");

    for required in [
        "set -eu",
        "github.com",
        "hyperliquid-cli",
        "hyperliquid",
        "uname -s",
        "uname -m",
    ] {
        assert!(
            install_script.contains(required),
            "install.sh is missing {required}"
        );
    }
}

#[test]
fn top_level_help_lists_all_release_command_groups() {
    let mut assertion = Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("--help")
        .assert()
        .success();

    for command in [
        "perps",
        "spot",
        "book",
        "orders",
        "positions",
        "transfers",
        "subaccounts",
        "subaccount",
        "account",
        "api-wallets",
        "api-wallet",
        "wallet",
        "staking",
        "vaults",
        "borrowlend",
        "prio",
        "subscribe",
        "feedback",
        "status",
        "setup",
    ] {
        assertion = assertion.stdout(predicate::str::contains(command));
    }
}

#[test]
fn live_command_dispatch_has_no_unclassified_local_signer_bypasses() {
    let registry = CommandRegistry::load().unwrap();

    let path = &["prio", "bid"][..];
    let command = registry.find_path(path).unwrap();
    assert_eq!(
        command.ows_signer,
        OwsSupport::LocalOnly,
        "{path:?} must advertise local-only signer support"
    );
}

#[test]
fn tool_catalog_and_registry_cover_the_same_command_inventory() {
    let catalog_text = repo_file("src/command_catalog.json");
    let catalog: serde_json::Value = serde_json::from_str(&catalog_text).unwrap();
    let registry = CommandRegistry::load().unwrap();
    let catalog_commands = catalog["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|command| command["command"].as_str().unwrap())
        .collect::<Vec<_>>();
    let registry_commands = registry
        .entries()
        .iter()
        .map(|command| command.command.as_str())
        .collect::<Vec<_>>();

    assert_eq!(registry_commands, catalog_commands);
}

#[test]
fn tool_catalog_materializes_registry_policy_metadata() {
    let catalog_text = repo_file("src/command_catalog.json");
    let catalog: serde_json::Value = serde_json::from_str(&catalog_text).unwrap();
    let registry = CommandRegistry::load().unwrap();
    let commands = catalog["commands"].as_array().unwrap();

    for catalog_command in commands {
        let command_name = catalog_command["command"].as_str().unwrap();
        let registry_command = registry
            .entries()
            .iter()
            .find(|command| command.command == command_name)
            .unwrap_or_else(|| panic!("catalog command missing from registry: {command_name}"));
        let registry_value = serde_json::to_value(registry_command).unwrap();

        for field in [
            "lifecycle",
            "risk",
            "dry_run",
            "raw_payload",
            "confirmation",
            "ows_signer",
        ] {
            assert_eq!(
                catalog_command[field], registry_value[field],
                "{command_name} must explicitly materialize registry metadata field {field}"
            );
        }

        for registry_input in &registry_command.inputs {
            let Some(kind) = registry_input.kind else {
                continue;
            };
            let catalog_args = catalog_command["args"].as_array().unwrap();
            let catalog_arg = catalog_args
                .iter()
                .find(|arg| arg["id"].as_str() == Some(registry_input.id.as_str()))
                .unwrap_or_else(|| {
                    panic!(
                        "{command_name} catalog is missing input {}",
                        registry_input.id
                    )
                });
            assert_eq!(
                catalog_arg["input_kind"],
                kind.as_str(),
                "{command_name} input {} must explicitly materialize input_kind",
                registry_input.id
            );
        }
    }
}
