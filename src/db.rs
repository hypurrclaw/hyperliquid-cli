//! SQLite-backed account storage.
//!
//! Private keys are encrypted before insertion and decrypted only when a signer
//! is explicitly resolved. The database never stores plaintext private keys.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use rand::TryRngCore;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::errors::CliError;

const ENCRYPTION_VERSION: &str = "v1";
const ACCOUNT_KEY_PASSPHRASE_ENV: &str = "HYPERLIQUID_ACCOUNT_KEY_PASSPHRASE";
const ACCOUNT_KEYCHAIN_DISABLED_ENV: &str = "HYPERLIQUID_ACCOUNT_KEYCHAIN_DISABLED";
const LEGACY_TEST_KEY_STORE_DIR_ENV: &str = "HYPERLIQUID_ACCOUNT_KEY_STORE_DIR";
const TEST_KEY_STORE_FILE_NAME: &str = "account-data.key";
const KEYCHAIN_SERVICE: &str = "hyperliquid-cli";
const KEYCHAIN_USER: &str = "accounts-data-encryption-key";
const PASSPHRASE_DOMAIN: &[u8] = b"hyperliquid-cli account encryption passphrase v1";

/// Storage backend for the account data-encryption key material.
///
/// Production builds use the OS keychain where available. Tests, CI, and
/// headless systems can inject a deterministic passphrase-derived key via
/// `HYPERLIQUID_ACCOUNT_KEY_PASSPHRASE` without persisting raw key material.
pub trait EncryptionKeyStore {
    fn load_key_material(&self) -> Result<Option<Vec<u8>>, anyhow::Error>;
    fn store_key_material(&self, material: &[u8]) -> Result<(), anyhow::Error>;
    fn delete_key_material(&self) -> Result<(), anyhow::Error>;
}

#[derive(Debug, Clone, Copy)]
pub struct AgentAccountMetadata<'a> {
    pub master_address: &'a str,
    pub agent_name: Option<&'a str>,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone)]
struct PassphraseEncryptionKeyStore {
    material: Vec<u8>,
}

impl PassphraseEncryptionKeyStore {
    fn new(passphrase: String) -> Result<Self, anyhow::Error> {
        let passphrase = passphrase.trim_end_matches(['\r', '\n']);
        if passphrase.is_empty() {
            return Err(CliError::Configuration(format!(
                "{ACCOUNT_KEY_PASSPHRASE_ENV} must not be empty"
            ))
            .into());
        }

        let mut hasher = Sha256::new();
        hasher.update(PASSPHRASE_DOMAIN);
        hasher.update([0]);
        hasher.update(passphrase.as_bytes());
        Ok(Self {
            material: hasher.finalize().to_vec(),
        })
    }
}

impl EncryptionKeyStore for PassphraseEncryptionKeyStore {
    fn load_key_material(&self) -> Result<Option<Vec<u8>>, anyhow::Error> {
        Ok(Some(self.material.clone()))
    }

    fn store_key_material(&self, _material: &[u8]) -> Result<(), anyhow::Error> {
        Ok(())
    }

    fn delete_key_material(&self) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct UnavailableEncryptionKeyStore;

impl EncryptionKeyStore for UnavailableEncryptionKeyStore {
    fn load_key_material(&self) -> Result<Option<Vec<u8>>, anyhow::Error> {
        Ok(None)
    }

    fn store_key_material(&self, _material: &[u8]) -> Result<(), anyhow::Error> {
        Err(secure_key_store_unavailable_error().into())
    }

    fn delete_key_material(&self) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct FileEncryptionKeyStore {
    path: PathBuf,
}

impl FileEncryptionKeyStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl EncryptionKeyStore for FileEncryptionKeyStore {
    fn load_key_material(&self) -> Result<Option<Vec<u8>>, anyhow::Error> {
        if !self.path.exists() {
            return Ok(None);
        }
        read_key_material_file(&self.path).map(Some)
    }

    fn store_key_material(&self, material: &[u8]) -> Result<(), anyhow::Error> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_secret_file(&self.path, BASE64.encode(material))
    }

    fn delete_key_material(&self) -> Result<(), anyhow::Error> {
        if self.path.exists() {
            std::fs::remove_file(&self.path).map_err(|err| {
                CliError::Configuration(format!(
                    "failed to remove account encryption key {}: {err}",
                    self.path.display()
                ))
            })?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct KeychainEncryptionKeyStore;

impl EncryptionKeyStore for KeychainEncryptionKeyStore {
    fn load_key_material(&self) -> Result<Option<Vec<u8>>, anyhow::Error> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).map_err(|err| {
            CliError::Configuration(format!("failed to access OS keychain: {err}"))
        })?;
        match entry.get_password() {
            Ok(secret) => BASE64.decode(secret.trim()).map(Some).map_err(|err| {
                CliError::Configuration(format!(
                    "invalid account encryption key in OS keychain: {err}"
                ))
                .into()
            }),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(CliError::Configuration(format!(
                "failed to read account encryption key from OS keychain: {err}"
            ))
            .into()),
        }
    }

    fn store_key_material(&self, material: &[u8]) -> Result<(), anyhow::Error> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).map_err(|err| {
            CliError::Configuration(format!("failed to access OS keychain: {err}"))
        })?;
        entry.set_password(&BASE64.encode(material)).map_err(|err| {
            CliError::Configuration(format!(
                "failed to store account encryption key in OS keychain: {err}"
            ))
            .into()
        })
    }

    fn delete_key_material(&self) -> Result<(), anyhow::Error> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).map_err(|err| {
            CliError::Configuration(format!("failed to access OS keychain: {err}"))
        })?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(CliError::Configuration(format!(
                "failed to remove account encryption key from OS keychain: {err}"
            ))
            .into()),
        }
    }
}

#[derive(Debug, Clone)]
struct DefaultEncryptionKeyStore {
    keychain: KeychainEncryptionKeyStore,
}

impl EncryptionKeyStore for DefaultEncryptionKeyStore {
    fn load_key_material(&self) -> Result<Option<Vec<u8>>, anyhow::Error> {
        match self.keychain.load_key_material() {
            Ok(Some(material)) => Ok(Some(material)),
            Ok(None) => Ok(None),
            Err(err) => prompt_passphrase_key_store_after_keychain_error(err)?.load_key_material(),
        }
    }

    fn store_key_material(&self, material: &[u8]) -> Result<(), anyhow::Error> {
        self.keychain
            .store_key_material(material)
            .map_err(|_| secure_key_store_unavailable_error().into())
    }

    fn delete_key_material(&self) -> Result<(), anyhow::Error> {
        self.keychain.delete_key_material()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: i64,
    pub alias: String,
    pub address: String,
    pub encrypted_private_key: String,
    pub account_type: String,
    pub is_default: bool,
    pub created_at: String,
    pub master_address: Option<String>,
    pub agent_name: Option<String>,
    pub expires_at: Option<u64>,
}

#[derive(Debug)]
pub struct AccountStore {
    conn: Connection,
    encryption_key: [u8; 32],
}

impl AccountStore {
    pub fn open_default() -> Result<Self, anyhow::Error> {
        let db_path = accounts_db_path()
            .ok_or_else(|| CliError::Configuration("Cannot determine data directory".into()))?;
        let create_if_missing = !db_path.exists();
        let key_store = default_encryption_key_store()?;
        Self::open_with_key_store_create(
            db_path,
            key_store.as_ref(),
            legacy_encryption_key_path().as_deref(),
            create_if_missing,
        )
    }

    pub fn open_existing_default() -> Result<Option<Self>, anyhow::Error> {
        let db_path = accounts_db_path()
            .ok_or_else(|| CliError::Configuration("Cannot determine data directory".into()))?;
        if !db_path.exists() {
            return Ok(None);
        }
        let key_store = default_encryption_key_store()?;
        Ok(Some(Self::open_read_only_with_key_store(
            db_path,
            key_store.as_ref(),
            legacy_encryption_key_path().as_deref(),
        )?))
    }

    pub fn open(
        db_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) -> Result<Self, anyhow::Error> {
        let key_store = FileEncryptionKeyStore::new(key_path.as_ref().to_path_buf());
        Self::open_with_key_store(db_path, &key_store, None)
    }

    pub fn open_with_key_store(
        db_path: impl AsRef<Path>,
        key_store: &dyn EncryptionKeyStore,
        legacy_key_path: Option<&Path>,
    ) -> Result<Self, anyhow::Error> {
        Self::open_with_key_store_create(db_path, key_store, legacy_key_path, true)
    }

    fn open_with_key_store_create(
        db_path: impl AsRef<Path>,
        key_store: &dyn EncryptionKeyStore,
        legacy_key_path: Option<&Path>,
        create_key_if_missing: bool,
    ) -> Result<Self, anyhow::Error> {
        let db_path = db_path.as_ref();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                CliError::Configuration(format!(
                    "failed to create data directory {}: {err}",
                    parent.display()
                ))
            })?;
        }
        let encryption_key =
            load_encryption_key(key_store, legacy_key_path, create_key_if_missing)?;

        let conn = Connection::open(db_path).map_err(|err| {
            CliError::Configuration(format!("failed to open accounts database: {err}"))
        })?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                alias TEXT NOT NULL UNIQUE,
                address TEXT NOT NULL,
                encrypted_private_key TEXT NOT NULL,
                type TEXT NOT NULL,
                is_default INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                master_address TEXT,
                agent_name TEXT,
                expires_at INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_accounts_default ON accounts(is_default);
            "#,
        )?;
        ensure_account_metadata_columns(&conn)?;

        Ok(Self {
            conn,
            encryption_key,
        })
    }

    pub fn open_read_only_with_key_store(
        db_path: impl AsRef<Path>,
        key_store: &dyn EncryptionKeyStore,
        legacy_key_path: Option<&Path>,
    ) -> Result<Self, anyhow::Error> {
        let db_path = db_path.as_ref();
        let conn = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|err| {
            CliError::Configuration(format!("failed to open accounts database: {err}"))
        })?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Self {
            conn,
            encryption_key: load_encryption_key_read_only(key_store, legacy_key_path)?,
        })
    }

    pub fn add_account(
        &mut self,
        alias: &str,
        address: &str,
        private_key: &str,
        account_type: &str,
        make_default: bool,
    ) -> Result<Account, anyhow::Error> {
        self.add_account_with_metadata(
            alias,
            address,
            private_key,
            account_type,
            make_default,
            AccountMetadata::default(),
        )
    }

    pub fn add_agent_account(
        &mut self,
        alias: &str,
        address: &str,
        private_key: &str,
        metadata: AgentAccountMetadata<'_>,
        make_default: bool,
    ) -> Result<Account, anyhow::Error> {
        self.add_account_with_metadata(
            alias,
            address,
            private_key,
            "agent-wallet",
            make_default,
            AccountMetadata {
                master_address: Some(metadata.master_address.trim().to_string()),
                agent_name: metadata
                    .agent_name
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_string),
                expires_at: metadata.expires_at,
            },
        )
    }

    fn add_account_with_metadata(
        &mut self,
        alias: &str,
        address: &str,
        private_key: &str,
        account_type: &str,
        make_default: bool,
        metadata: AccountMetadata,
    ) -> Result<Account, anyhow::Error> {
        let alias = alias.trim();
        if alias.is_empty() {
            return Err(CliError::Unsupported("account alias cannot be empty".into()).into());
        }
        let account_type = account_type.trim();
        if account_type.is_empty() {
            return Err(CliError::Unsupported("account type cannot be empty".into()).into());
        }
        if metadata
            .master_address
            .as_deref()
            .is_some_and(|address| address.trim().is_empty())
        {
            return Err(
                CliError::Unsupported("agent master address cannot be empty".into()).into(),
            );
        }

        let tx = self.conn.transaction()?;
        if make_default {
            tx.execute("UPDATE accounts SET is_default = 0", [])?;
        }

        let encrypted_private_key = encrypt_private_key(private_key, &self.encryption_key)?;
        let created_at = chrono::Utc::now().to_rfc3339();
        tx.execute(
            "INSERT INTO accounts (
                alias, address, encrypted_private_key, type, is_default, created_at,
                master_address, agent_name, expires_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                alias,
                address,
                encrypted_private_key,
                account_type,
                if make_default { 1 } else { 0 },
                created_at,
                metadata.master_address.as_deref(),
                metadata.agent_name.as_deref(),
                metadata.expires_at.map(|value| value as i64),
            ],
        )
        .map_err(|err| {
            if is_unique_constraint(&err) {
                CliError::Unsupported(format!("account alias '{alias}' already exists")).into()
            } else {
                anyhow::Error::new(err)
            }
        })?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        self.account_by_id(id)?
            .ok_or_else(|| anyhow::anyhow!("inserted account could not be loaded"))
    }

    pub fn list_accounts(&self) -> Result<Vec<Account>, anyhow::Error> {
        let projection = account_select_projection(&self.conn)?;
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {projection} FROM accounts ORDER BY is_default DESC, alias ASC"
        ))?;
        let rows = stmt.query_map([], row_to_account)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn default_account(&self) -> Result<Option<Account>, anyhow::Error> {
        let projection = account_select_projection(&self.conn)?;
        self.conn
            .query_row(
                &format!(
                    "SELECT {projection} FROM accounts WHERE is_default = 1 ORDER BY id LIMIT 1"
                ),
                [],
                row_to_account,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn first_account(&self) -> Result<Option<Account>, anyhow::Error> {
        let projection = account_select_projection(&self.conn)?;
        self.conn
            .query_row(
                &format!("SELECT {projection} FROM accounts ORDER BY id LIMIT 1"),
                [],
                row_to_account,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn default_or_first_account(&self) -> Result<Option<Account>, anyhow::Error> {
        match self.default_account()? {
            Some(account) => Ok(Some(account)),
            None => self.first_account(),
        }
    }

    pub fn account_by_selector(&self, selector: &str) -> Result<Option<Account>, anyhow::Error> {
        if let Ok(id) = selector.trim().parse::<i64>()
            && let Some(account) = self.account_by_id(id)?
        {
            return Ok(Some(account));
        }

        let projection = account_select_projection(&self.conn)?;
        self.conn
            .query_row(
                &format!(
                    "SELECT {projection} FROM accounts WHERE alias = ?1 OR address = ?1 LIMIT 1"
                ),
                params![selector.trim()],
                row_to_account,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn account_alias_exists(&self, alias: &str) -> Result<bool, anyhow::Error> {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE alias = ?1)",
                params![alias.trim()],
                |row| row.get::<_, i64>(0),
            )
            .map(|exists| exists != 0)
            .map_err(Into::into)
    }

    pub fn set_default(&mut self, selector: &str) -> Result<Account, anyhow::Error> {
        let account = self
            .account_by_selector(selector)?
            .ok_or_else(|| CliError::Unsupported(format!("account '{selector}' not found")))?;
        let tx = self.conn.transaction()?;
        tx.execute("UPDATE accounts SET is_default = 0", [])?;
        tx.execute(
            "UPDATE accounts SET is_default = 1 WHERE id = ?1",
            params![account.id],
        )?;
        tx.commit()?;
        self.account_by_id(account.id)?
            .ok_or_else(|| anyhow::anyhow!("default account could not be loaded"))
    }

    pub fn remove_account(&mut self, selector: &str) -> Result<Account, anyhow::Error> {
        let account = self
            .account_by_selector(selector)?
            .ok_or_else(|| CliError::Unsupported(format!("account '{selector}' not found")))?;
        self.conn
            .execute("DELETE FROM accounts WHERE id = ?1", params![account.id])?;
        if account.is_default
            && let Some(next) = self.first_account()?
        {
            self.set_default(&next.id.to_string())?;
        }
        Ok(account)
    }

    pub fn clear_accounts(&mut self) -> Result<(), anyhow::Error> {
        self.conn.execute("DELETE FROM accounts", [])?;
        Ok(())
    }

    pub fn decrypt_account_private_key(&self, account: &Account) -> Result<String, anyhow::Error> {
        decrypt_private_key(&account.encrypted_private_key, &self.encryption_key)
    }

    fn account_by_id(&self, id: i64) -> Result<Option<Account>, anyhow::Error> {
        let projection = account_select_projection(&self.conn)?;
        self.conn
            .query_row(
                &format!("SELECT {projection} FROM accounts WHERE id = ?1"),
                params![id],
                row_to_account,
            )
            .optional()
            .map_err(Into::into)
    }
}

pub fn accounts_db_path() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("hyperliquid").join("accounts.db"))
}

pub fn legacy_encryption_key_path() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("hyperliquid").join("accounts.key"))
}

pub fn deprecated_fallback_encryption_key_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("hyperliquid").join(TEST_KEY_STORE_FILE_NAME))
}

fn default_encryption_key_store() -> Result<Box<dyn EncryptionKeyStore>, anyhow::Error> {
    if std::env::var_os(LEGACY_TEST_KEY_STORE_DIR_ENV).is_some() {
        return Err(CliError::Configuration(format!(
            "{LEGACY_TEST_KEY_STORE_DIR_ENV} is no longer supported because it persists raw account encryption keys; use {ACCOUNT_KEY_PASSPHRASE_ENV} for deterministic secure automation"
        ))
        .into());
    }

    if let Some(passphrase) = std::env::var_os(ACCOUNT_KEY_PASSPHRASE_ENV) {
        return Ok(Box::new(PassphraseEncryptionKeyStore::new(
            passphrase.to_string_lossy().into_owned(),
        )?));
    }

    if std::env::var_os(ACCOUNT_KEYCHAIN_DISABLED_ENV).is_some() {
        return Ok(Box::new(UnavailableEncryptionKeyStore));
    }

    Ok(Box::new(DefaultEncryptionKeyStore {
        keychain: KeychainEncryptionKeyStore,
    }))
}

pub fn delete_default_encryption_key() -> Result<(), anyhow::Error> {
    let key_store = default_encryption_key_store()?;
    key_store.delete_key_material()?;
    if let Some(path) = deprecated_fallback_encryption_key_path()
        && path.exists()
    {
        std::fs::remove_file(&path).map_err(|err| {
            CliError::Configuration(format!(
                "failed to remove deprecated raw account encryption key {}: {err}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

fn row_to_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: row.get(0)?,
        alias: row.get(1)?,
        address: row.get(2)?,
        encrypted_private_key: row.get(3)?,
        account_type: row.get(4)?,
        is_default: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        master_address: row.get(7)?,
        agent_name: row.get(8)?,
        expires_at: row.get::<_, Option<i64>>(9)?.map(|value| value as u64),
    })
}

#[derive(Debug, Clone, Default)]
struct AccountMetadata {
    master_address: Option<String>,
    agent_name: Option<String>,
    expires_at: Option<u64>,
}

const ACCOUNT_METADATA_COLUMNS: [(&str, &str); 3] = [
    ("master_address", "TEXT"),
    ("agent_name", "TEXT"),
    ("expires_at", "INTEGER"),
];

fn account_select_projection(conn: &Connection) -> Result<String, anyhow::Error> {
    let missing_columns = missing_account_metadata_columns(conn)?;
    let has_column = |column: &str| {
        !missing_columns
            .iter()
            .any(|(missing_column, _)| *missing_column == column)
    };
    let master_address = if has_column("master_address") {
        "master_address"
    } else {
        "NULL AS master_address"
    };
    let agent_name = if has_column("agent_name") {
        "agent_name"
    } else {
        "NULL AS agent_name"
    };
    let expires_at = if has_column("expires_at") {
        "expires_at"
    } else {
        "NULL AS expires_at"
    };

    Ok(format!(
        "id, alias, address, encrypted_private_key, type, is_default, created_at, {master_address}, {agent_name}, {expires_at}"
    ))
}

fn ensure_account_metadata_columns(conn: &Connection) -> Result<(), anyhow::Error> {
    for (column, ty) in missing_account_metadata_columns(conn)? {
        conn.execute(
            &format!("ALTER TABLE accounts ADD COLUMN {column} {ty}"),
            [],
        )?;
    }
    Ok(())
}

fn missing_account_metadata_columns(
    conn: &Connection,
) -> Result<Vec<(&'static str, &'static str)>, anyhow::Error> {
    let mut stmt = conn.prepare("PRAGMA table_info(accounts)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;

    if columns.is_empty() {
        return Ok(Vec::new());
    }

    Ok(ACCOUNT_METADATA_COLUMNS
        .into_iter()
        .filter(|(column, _)| !columns.iter().any(|existing| existing == column))
        .collect())
}

fn is_unique_constraint(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(error, _)
            if error.code == rusqlite::ErrorCode::ConstraintViolation
    )
}

fn load_encryption_key(
    key_store: &dyn EncryptionKeyStore,
    legacy_key_path: Option<&Path>,
    create_if_missing: bool,
) -> Result<[u8; 32], anyhow::Error> {
    if let Some(material) = key_store.load_key_material()? {
        return Ok(derive_key(&material));
    }

    if let Some(path) = legacy_key_path
        && path.exists()
    {
        let material = read_key_material_file(path)?;
        key_store.store_key_material(&material)?;
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(CliError::Configuration(format!(
                    "failed to remove legacy account encryption key {}: {err}",
                    path.display()
                ))
                .into());
            }
        }
        return Ok(derive_key(&material));
    }

    if !create_if_missing {
        return Err(CliError::InvalidAuth(
            "stored account encryption key is unavailable; cannot decrypt accounts from the data directory alone"
                .into(),
        )
        .into());
    }

    let mut material = [0_u8; 32];
    rand::rngs::OsRng.try_fill_bytes(&mut material)?;
    if let Err(err) = key_store.store_key_material(&material) {
        if std::io::stdin().is_terminal() {
            let passphrase_store = prompt_passphrase_key_store_after_keychain_error(err)?;
            let material = passphrase_store
                .load_key_material()?
                .ok_or_else(secure_key_store_unavailable_error)?;
            return Ok(derive_key(&material));
        }
        return Err(err);
    }
    Ok(derive_key(&material))
}

fn load_encryption_key_read_only(
    key_store: &dyn EncryptionKeyStore,
    legacy_key_path: Option<&Path>,
) -> Result<[u8; 32], anyhow::Error> {
    if let Some(material) = key_store.load_key_material()? {
        return Ok(derive_key(&material));
    }

    if let Some(path) = legacy_key_path
        && path.exists()
    {
        return read_key_material_file(path).map(|material| derive_key(&material));
    }

    Err(CliError::InvalidAuth(
        "stored account encryption key is unavailable; cannot decrypt accounts from the data directory alone"
            .into(),
    )
    .into())
}

fn prompt_passphrase_key_store_after_keychain_error(
    keychain_error: anyhow::Error,
) -> Result<PassphraseEncryptionKeyStore, anyhow::Error> {
    if !std::io::stdin().is_terminal() {
        return Err(secure_key_store_unavailable_error_with_cause(&keychain_error).into());
    }

    eprintln!(
        "OS keychain is unavailable ({keychain_error}). Using a passphrase-derived account encryption key instead."
    );
    let passphrase = rpassword::prompt_password("Account encryption passphrase: ")?;
    PassphraseEncryptionKeyStore::new(passphrase)
}

fn secure_key_store_unavailable_error() -> CliError {
    CliError::Configuration(format!(
        "OS keychain is unavailable for account encryption keys. No raw fallback key file will be written. Set {ACCOUNT_KEY_PASSPHRASE_ENV} for secure passphrase-derived automation or run interactively to enter an account encryption passphrase."
    ))
}

fn secure_key_store_unavailable_error_with_cause(cause: &anyhow::Error) -> CliError {
    CliError::Configuration(format!(
        "OS keychain is unavailable for account encryption keys ({cause}). No raw fallback key file will be written. Set {ACCOUNT_KEY_PASSPHRASE_ENV} for secure passphrase-derived automation or run interactively to enter an account encryption passphrase."
    ))
}

fn read_key_material_file(path: &Path) -> Result<Vec<u8>, anyhow::Error> {
    let encoded = std::fs::read_to_string(path).map_err(|err| {
        CliError::Configuration(format!(
            "failed to read account encryption key {}: {err}",
            path.display()
        ))
    })?;
    BASE64.decode(encoded.trim()).map_err(|err| {
        CliError::Configuration(format!(
            "invalid account encryption key {}: {err}",
            path.display()
        ))
        .into()
    })
}

fn write_secret_file(path: &Path, contents: String) -> Result<(), anyhow::Error> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(contents.as_bytes())?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, contents)?;
        Ok(())
    }
}

fn derive_key(material: &[u8]) -> [u8; 32] {
    Sha256::digest(material).into()
}

pub fn encrypt_private_key(private_key: &str, key: &[u8; 32]) -> Result<String, anyhow::Error> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|err| anyhow::anyhow!("failed to initialize encryption: {err}"))?;
    let mut nonce_bytes = [0_u8; 12];
    rand::rngs::OsRng.try_fill_bytes(&mut nonce_bytes)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, private_key.as_bytes())
        .map_err(|err| anyhow::anyhow!("failed to encrypt private key: {err}"))?;
    Ok(format!(
        "{ENCRYPTION_VERSION}:{}:{}",
        BASE64.encode(nonce_bytes),
        BASE64.encode(ciphertext)
    ))
}

pub fn decrypt_private_key(encrypted: &str, key: &[u8; 32]) -> Result<String, anyhow::Error> {
    let mut parts = encrypted.split(':');
    let version = parts.next().unwrap_or_default();
    let nonce = parts.next().unwrap_or_default();
    let ciphertext = parts.next().unwrap_or_default();
    if version != ENCRYPTION_VERSION || parts.next().is_some() {
        return Err(
            CliError::InvalidAuth("stored account key has unsupported format".into()).into(),
        );
    }
    let nonce = BASE64.decode(nonce).map_err(|_| {
        CliError::InvalidAuth("stored account key has invalid nonce encoding".into())
    })?;
    if nonce.len() != 12 {
        return Err(
            CliError::InvalidAuth("stored account key has invalid nonce length".into()).into(),
        );
    }
    let ciphertext = BASE64.decode(ciphertext).map_err(|_| {
        CliError::InvalidAuth("stored account key has invalid ciphertext encoding".into())
    })?;
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|err| anyhow::anyhow!("failed to initialize decryption: {err}"))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| CliError::InvalidAuth("stored account key could not be decrypted".into()))?;
    String::from_utf8(plaintext)
        .map_err(|_| CliError::InvalidAuth("stored account key is not valid UTF-8".into()).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use tempfile::TempDir;

    const KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000007";

    #[derive(Clone, Default)]
    struct InMemoryKeyStore {
        material: Rc<RefCell<Option<Vec<u8>>>>,
    }

    impl EncryptionKeyStore for InMemoryKeyStore {
        fn load_key_material(&self) -> Result<Option<Vec<u8>>, anyhow::Error> {
            Ok(self.material.borrow().clone())
        }

        fn store_key_material(&self, material: &[u8]) -> Result<(), anyhow::Error> {
            *self.material.borrow_mut() = Some(material.to_vec());
            Ok(())
        }

        fn delete_key_material(&self) -> Result<(), anyhow::Error> {
            *self.material.borrow_mut() = None;
            Ok(())
        }
    }

    fn store() -> (TempDir, AccountStore) {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("accounts.db");
        let key = tmp.path().join("accounts.key");
        let store = AccountStore::open(db, key).unwrap();
        (tmp, store)
    }

    #[test]
    fn account_keys_are_encrypted_and_decryptable() {
        let (_tmp, mut store) = store();
        let account = store
            .add_account("main", "0xabc", KEY, "api-wallet", true)
            .unwrap();

        assert_ne!(account.encrypted_private_key, KEY);
        assert!(
            !account
                .encrypted_private_key
                .contains(KEY.trim_start_matches("0x"))
        );
        assert_eq!(store.decrypt_account_private_key(&account).unwrap(), KEY);
    }

    #[test]
    fn injected_key_store_can_decrypt_but_data_directory_alone_cannot() {
        let tmp = TempDir::new().unwrap();
        let db = tmp
            .path()
            .join("data")
            .join("hyperliquid")
            .join("accounts.db");
        let key_store = InMemoryKeyStore::default();
        let mut store = AccountStore::open_with_key_store(&db, &key_store, None).unwrap();
        let account = store
            .add_account("main", "0xabc", KEY, "api-wallet", true)
            .unwrap();
        assert_eq!(store.decrypt_account_private_key(&account).unwrap(), KEY);
        drop(store);

        let reopened = AccountStore::open_read_only_with_key_store(&db, &key_store, None).unwrap();
        let account = reopened.default_account().unwrap().unwrap();
        assert_eq!(reopened.decrypt_account_private_key(&account).unwrap(), KEY);

        let empty_key_store = InMemoryKeyStore::default();
        let err =
            AccountStore::open_read_only_with_key_store(&db, &empty_key_store, None).unwrap_err();
        assert!(err.to_string().contains("data directory alone"));
    }

    #[test]
    fn legacy_data_dir_key_is_migrated_to_injected_store_and_removed_on_writable_open() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path().join("data").join("hyperliquid");
        let db = data_dir.join("accounts.db");
        let legacy_key = data_dir.join("accounts.key");
        let mut legacy_store = AccountStore::open(&db, &legacy_key).unwrap();
        let account = legacy_store
            .add_account("main", "0xabc", KEY, "api-wallet", true)
            .unwrap();
        assert_eq!(
            legacy_store.decrypt_account_private_key(&account).unwrap(),
            KEY
        );
        drop(legacy_store);

        let injected_store = InMemoryKeyStore::default();
        let migrated =
            AccountStore::open_with_key_store(&db, &injected_store, Some(&legacy_key)).unwrap();
        let account = migrated.default_account().unwrap().unwrap();
        assert_eq!(migrated.decrypt_account_private_key(&account).unwrap(), KEY);
        assert!(!legacy_key.exists());
        assert!(injected_store.load_key_material().unwrap().is_some());
    }

    #[test]
    fn read_only_open_uses_legacy_key_without_migrating_or_removing_it() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path().join("data").join("hyperliquid");
        let db = data_dir.join("accounts.db");
        let legacy_key = data_dir.join("accounts.key");
        let mut legacy_store = AccountStore::open(&db, &legacy_key).unwrap();
        let created = legacy_store
            .add_account("main", "0xabc", KEY, "api-wallet", true)
            .unwrap();
        assert_eq!(
            legacy_store.decrypt_account_private_key(&created).unwrap(),
            KEY
        );
        drop(legacy_store);

        let injected_store = InMemoryKeyStore::default();
        let reopened =
            AccountStore::open_read_only_with_key_store(&db, &injected_store, Some(&legacy_key))
                .unwrap();
        let account = reopened.default_account().unwrap().unwrap();
        assert_eq!(reopened.decrypt_account_private_key(&account).unwrap(), KEY);

        assert!(legacy_key.exists());
        assert!(injected_store.load_key_material().unwrap().is_none());
    }

    #[test]
    fn read_only_open_preserves_legacy_account_schema() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("accounts.db");
        let key_store = InMemoryKeyStore::default();
        let mut store = AccountStore::open_with_key_store(&db, &key_store, None).unwrap();
        let created = store
            .add_account("main", "0xabc", KEY, "api-wallet", true)
            .unwrap();
        assert_eq!(store.decrypt_account_private_key(&created).unwrap(), KEY);
        drop(store);

        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            r#"
            ALTER TABLE accounts RENAME TO accounts_new_schema;
            CREATE TABLE accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                alias TEXT NOT NULL UNIQUE,
                address TEXT NOT NULL,
                encrypted_private_key TEXT NOT NULL,
                type TEXT NOT NULL,
                is_default INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );
            INSERT INTO accounts (
                id, alias, address, encrypted_private_key, type, is_default, created_at
            )
            SELECT id, alias, address, encrypted_private_key, type, is_default, created_at
            FROM accounts_new_schema;
            DROP TABLE accounts_new_schema;
            CREATE INDEX IF NOT EXISTS idx_accounts_default ON accounts(is_default);
            "#,
        )
        .unwrap();
        drop(conn);

        let reopened = AccountStore::open_read_only_with_key_store(&db, &key_store, None).unwrap();
        let account = reopened.account_by_selector("main").unwrap().unwrap();
        assert_eq!(account.master_address, None);
        assert_eq!(account.agent_name, None);
        assert_eq!(account.expires_at, None);
        assert_eq!(reopened.decrypt_account_private_key(&account).unwrap(), KEY);

        let conn = Connection::open(&db).unwrap();
        let columns = account_table_columns(&conn);
        assert!(!columns.iter().any(|column| column == "master_address"));
        assert!(!columns.iter().any(|column| column == "agent_name"));
        assert!(!columns.iter().any(|column| column == "expires_at"));
    }

    #[test]
    fn only_one_account_is_default() {
        let (_tmp, mut store) = store();
        store
            .add_account("main", "0xabc", KEY, "api-wallet", true)
            .unwrap();
        store
            .add_account(
                "backup",
                "0xdef",
                "0x0000000000000000000000000000000000000000000000000000000000000008",
                "api-wallet",
                true,
            )
            .unwrap();

        let accounts = store.list_accounts().unwrap();
        assert_eq!(
            accounts.iter().filter(|account| account.is_default).count(),
            1
        );
        assert_eq!(store.default_account().unwrap().unwrap().alias, "backup");
    }

    #[test]
    fn set_default_and_remove_by_alias() {
        let (_tmp, mut store) = store();
        store
            .add_account("main", "0xabc", KEY, "api-wallet", true)
            .unwrap();
        store
            .add_account(
                "backup",
                "0xdef",
                "0x0000000000000000000000000000000000000000000000000000000000000008",
                "api-wallet",
                false,
            )
            .unwrap();

        let default = store.set_default("backup").unwrap();
        assert_eq!(default.alias, "backup");
        let removed = store.remove_account("main").unwrap();
        assert_eq!(removed.alias, "main");
        assert_eq!(store.list_accounts().unwrap().len(), 1);
    }

    #[test]
    fn decrypt_rejects_invalid_nonce_length_without_panicking() {
        let key = [7_u8; 32];
        let encrypted = format!(
            "{ENCRYPTION_VERSION}:{}:{}",
            BASE64.encode([1_u8; 8]),
            BASE64.encode([2_u8; 16])
        );

        let err = decrypt_private_key(&encrypted, &key).unwrap_err();
        assert!(err.to_string().contains("invalid nonce length"));
    }

    fn account_table_columns(conn: &Connection) -> Vec<String> {
        let mut stmt = conn.prepare("PRAGMA table_info(accounts)").unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }
}
