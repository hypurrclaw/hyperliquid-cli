//! Writes an `accounts.db` the way releases before the OWS vault did.
//!
//! Production code only reads this store, so the writer lives with the tests. The file
//! is shared by the unit tests (`src/legacy_accounts.rs`) and the integration tests
//! (`tests/support`); it must stay free of references to the crate under test.

use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

/// Raw data-encryption key material the fixture stores in the key file.
pub const KEY_MATERIAL: [u8; 32] = [7; 32];

pub struct Row<'a> {
    pub alias: &'a str,
    pub address: &'a str,
    pub private_key: &'a str,
    pub account_type: &'a str,
    pub is_default: bool,
    pub master_address: Option<&'a str>,
}

impl<'a> Row<'a> {
    pub fn new(alias: &'a str, address: &'a str, private_key: &'a str) -> Self {
        Self {
            alias,
            address,
            private_key,
            account_type: "api-wallet",
            is_default: false,
            master_address: None,
        }
    }

    pub fn default_account(mut self) -> Self {
        self.is_default = true;
        self
    }

    pub fn agent_of(mut self, master_address: &'a str) -> Self {
        self.account_type = "agent-wallet";
        self.master_address = Some(master_address);
        self
    }
}

/// Which `accounts` table layout to create.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Schema {
    Current,
    /// Before `master_address`, `agent_name` and `expires_at` existed.
    WithoutAgentColumns,
}

/// The AES key a store derives from raw key material.
pub fn derive_key(material: &[u8]) -> [u8; 32] {
    Sha256::digest(material).into()
}

pub fn write_key_file(path: &Path, material: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, BASE64.encode(material)).unwrap();
}

fn encrypt(private_key: &str, key: &[u8; 32], counter: u8) -> String {
    let cipher = Aes256Gcm::new_from_slice(key).unwrap();
    let mut nonce_bytes = [0_u8; 12];
    nonce_bytes[0] = counter;
    let ciphertext = cipher
        .encrypt(&Nonce::from(nonce_bytes), private_key.as_bytes())
        .unwrap();
    format!(
        "v1:{}:{}",
        BASE64.encode(nonce_bytes),
        BASE64.encode(ciphertext)
    )
}

/// Create `db` with `rows`, encrypting each key with the key derived from `material`.
pub fn create_accounts_db(db: &Path, material: &[u8], schema: Schema, rows: &[Row<'_>]) {
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let conn = Connection::open(db).unwrap();
    let agent_columns = match schema {
        Schema::Current => ", master_address TEXT, agent_name TEXT, expires_at INTEGER",
        Schema::WithoutAgentColumns => "",
    };
    conn.execute_batch(&format!(
        "CREATE TABLE accounts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            alias TEXT NOT NULL UNIQUE,
            address TEXT NOT NULL,
            encrypted_private_key TEXT NOT NULL,
            type TEXT NOT NULL,
            is_default INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL{agent_columns}
        );
        CREATE INDEX idx_accounts_default ON accounts(is_default);"
    ))
    .unwrap();
    let key = derive_key(material);
    for (index, row) in rows.iter().enumerate() {
        let encrypted = encrypt(row.private_key, &key, index as u8);
        conn.execute(
            "INSERT INTO accounts (alias, address, encrypted_private_key, type, is_default, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, '2026-01-01T00:00:00+00:00')",
            params![
                row.alias,
                row.address,
                encrypted,
                row.account_type,
                i64::from(row.is_default)
            ],
        )
        .unwrap();
        if let (Schema::Current, Some(master)) = (schema, row.master_address) {
            conn.execute(
                "UPDATE accounts SET master_address = ?1 WHERE alias = ?2",
                params![master, row.alias],
            )
            .unwrap();
        }
    }
}

/// Create `db` and the plain key file beside it that older releases used.
pub fn create_accounts_db_with_key_file(db: &Path, key_file: &Path, rows: &[Row<'_>]) {
    write_key_file(key_file, &KEY_MATERIAL);
    create_accounts_db(db, &KEY_MATERIAL, Schema::Current, rows);
}
