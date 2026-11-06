//! Read-only access to the encrypted SQLite account store older releases wrote.
//!
//! Wallets now live in the OWS vault and nothing writes `accounts.db` any more.
//! Existing databases keep working as a signer and address-lookup fallback:
//! `--account` and the default signer consult this store after the vault.
//! Private keys are decrypted only when a signer is resolved from a row.

use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hypersdk::Address;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::errors::CliError;
use crate::settings::{Settings, env};

const ENCRYPTION_VERSION: &str = "v1";
const KEYCHAIN_SERVICE: &str = "hyperliquid-cli";
const KEYCHAIN_USER: &str = "accounts-data-encryption-key";
const PASSPHRASE_DOMAIN: &[u8] = b"hyperliquid-cli account encryption passphrase v1";

/// Where the data-encryption key material comes from.
///
/// Production uses the OS keychain. Tests, CI and headless systems inject a
/// passphrase-derived key through `HYPERLIQUID_ACCOUNT_KEY_PASSPHRASE`.
#[derive(Debug, Clone)]
enum KeySource {
    Material(Vec<u8>),
    Unavailable,
    Keychain,
}

impl KeySource {
    fn from_settings(settings: &Settings) -> Result<Self, anyhow::Error> {
        let env = settings.env();
        if env.is_set(env::ACCOUNT_KEY_STORE_DIR) {
            return Err(CliError::Configuration(format!(
                "{} is no longer supported because it persists raw account encryption keys; use {} for deterministic secure automation",
                env::ACCOUNT_KEY_STORE_DIR,
                env::ACCOUNT_KEY_PASSPHRASE
            ))
            .into());
        }
        if let Some(passphrase) = env.get_lossy(env::ACCOUNT_KEY_PASSPHRASE) {
            return Self::from_passphrase(&passphrase);
        }
        if env.is_set(env::ACCOUNT_KEYCHAIN_DISABLED) {
            return Ok(Self::Unavailable);
        }
        Ok(Self::Keychain)
    }

    fn from_passphrase(passphrase: &str) -> Result<Self, anyhow::Error> {
        let passphrase = passphrase.trim_end_matches(['\r', '\n']);
        if passphrase.is_empty() {
            return Err(CliError::Configuration(format!(
                "{} must not be empty",
                env::ACCOUNT_KEY_PASSPHRASE
            ))
            .into());
        }
        let mut hasher = Sha256::new();
        hasher.update(PASSPHRASE_DOMAIN);
        hasher.update([0]);
        hasher.update(passphrase.as_bytes());
        Ok(Self::Material(hasher.finalize().to_vec()))
    }

    fn load(&self) -> Result<Option<Vec<u8>>, anyhow::Error> {
        match self {
            Self::Material(material) => Ok(Some(material.clone())),
            Self::Unavailable => Ok(None),
            Self::Keychain => match load_from_keychain() {
                Ok(material) => Ok(material),
                Err(err) => prompt_passphrase_after_keychain_error(&err)?.load(),
            },
        }
    }
}

fn load_from_keychain() -> Result<Option<Vec<u8>>, anyhow::Error> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
        .map_err(|err| CliError::Configuration(format!("failed to access OS keychain: {err}")))?;
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

fn prompt_passphrase_after_keychain_error(
    keychain_error: &anyhow::Error,
) -> Result<KeySource, anyhow::Error> {
    if !crate::mode::can_prompt() || !crate::output::terminal::stdin_is_terminal() {
        return Err(CliError::MissingInput(format!(
            "Can't unlock account encryption keys because the OS keychain is unavailable ({keychain_error}). Set {} to use a passphrase instead.",
            env::ACCOUNT_KEY_PASSPHRASE
        ))
        .into());
    }

    eprintln!(
        "OS keychain is unavailable ({keychain_error}). Using a passphrase-derived account encryption key instead."
    );
    let passphrase = crate::prompt::read_secret(
        "Account encryption passphrase: ",
        env::ACCOUNT_KEY_PASSPHRASE,
    )?;
    KeySource::from_passphrase(&passphrase)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyAccount {
    id: i64,
    pub alias: String,
    address: String,
    master_address: Option<String>,
    encrypted_private_key: String,
}

impl LegacyAccount {
    /// The address account data is queried for: the master address of an agent
    /// wallet, else the account's own address.
    pub fn query_address(&self) -> Result<Address, CliError> {
        let raw = self
            .master_address
            .as_deref()
            .unwrap_or(self.address.as_str());
        raw.parse::<Address>().map_err(|_| {
            CliError::Internal(anyhow::anyhow!(
                "stored account '{}' has invalid query address {}",
                self.alias,
                raw
            ))
        })
    }
}

pub struct LegacyAccounts {
    conn: Connection,
    has_master_address: bool,
    encryption_key: [u8; 32],
}

impl std::fmt::Debug for LegacyAccounts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LegacyAccounts").finish_non_exhaustive()
    }
}

impl LegacyAccounts {
    /// Open the default database for reading. `Ok(None)` when it doesn't exist.
    pub fn open_default(settings: &Settings) -> Result<Option<Self>, anyhow::Error> {
        let db_path = crate::paths::legacy_accounts_db()
            .ok_or_else(|| CliError::Configuration("Cannot determine data directory".into()))?;
        if !db_path.exists() {
            return Ok(None);
        }
        let key_source = KeySource::from_settings(settings)?;
        Self::open(
            &db_path,
            &key_source,
            crate::paths::legacy_accounts_key().as_deref(),
        )
        .map(Some)
    }

    /// The key file is a fallback for stores that predate the keychain. It is read
    /// but never migrated or removed.
    fn open(
        db_path: &Path,
        key_source: &KeySource,
        legacy_key_path: Option<&Path>,
    ) -> Result<Self, anyhow::Error> {
        let conn = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|err| {
            CliError::Configuration(format!("failed to open accounts database: {err}"))
        })?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let has_master_address = table_has_column(&conn, "master_address")?;
        Ok(Self {
            conn,
            has_master_address,
            encryption_key: load_encryption_key(key_source, legacy_key_path)?,
        })
    }

    fn select(&self) -> String {
        let master_address = if self.has_master_address {
            "master_address"
        } else {
            "NULL AS master_address"
        };
        format!("SELECT id, alias, address, encrypted_private_key, {master_address} FROM accounts")
    }

    fn query_one(
        &self,
        tail: &str,
        params: impl rusqlite::Params,
    ) -> Result<Option<LegacyAccount>, anyhow::Error> {
        self.conn
            .query_row(&format!("{} {tail}", self.select()), params, |row| {
                Ok(LegacyAccount {
                    id: row.get(0)?,
                    alias: row.get(1)?,
                    address: row.get(2)?,
                    encrypted_private_key: row.get(3)?,
                    master_address: row.get(4)?,
                })
            })
            .optional()
            .map_err(Into::into)
    }

    /// A numeric selector matches the account id first; otherwise the alias or address.
    pub fn account_by_selector(
        &self,
        selector: &str,
    ) -> Result<Option<LegacyAccount>, anyhow::Error> {
        let selector = selector.trim();
        if let Ok(id) = selector.parse::<i64>()
            && let Some(account) = self.query_one("WHERE id = ?1", params![id])?
        {
            return Ok(Some(account));
        }
        self.query_one(
            "WHERE alias = ?1 OR address = ?1 LIMIT 1",
            params![selector],
        )
    }

    /// The account marked default, else the oldest account.
    pub fn default_or_first_account(&self) -> Result<Option<LegacyAccount>, anyhow::Error> {
        match self.query_one("WHERE is_default = 1 ORDER BY id LIMIT 1", [])? {
            Some(account) => Ok(Some(account)),
            None => self.query_one("ORDER BY id LIMIT 1", []),
        }
    }

    pub fn decrypt_private_key(&self, account: &LegacyAccount) -> Result<String, anyhow::Error> {
        decrypt_private_key(&account.encrypted_private_key, &self.encryption_key)
    }
}

fn table_has_column(conn: &Connection, column: &str) -> Result<bool, anyhow::Error> {
    let mut stmt = conn.prepare("PRAGMA table_info(accounts)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(columns.iter().any(|existing| existing == column))
}

fn unavailable_key_error() -> CliError {
    CliError::InvalidAuth(
        "stored account encryption key is unavailable; cannot decrypt accounts from the data directory alone"
            .into(),
    )
}

fn load_encryption_key(
    key_source: &KeySource,
    legacy_key_path: Option<&Path>,
) -> Result<[u8; 32], anyhow::Error> {
    if let Some(material) = key_source.load()? {
        return Ok(derive_key(&material));
    }

    if let Some(path) = legacy_key_path
        && path.exists()
    {
        return read_key_material_file(path).map(|material| derive_key(&material));
    }

    Err(unavailable_key_error().into())
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

fn derive_key(material: &[u8]) -> [u8; 32] {
    Sha256::digest(material).into()
}

fn decrypt_private_key(encrypted: &str, key: &[u8; 32]) -> Result<String, anyhow::Error> {
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
    let nonce = Nonce::try_from(nonce.as_slice())
        .map_err(|_| CliError::InvalidAuth("stored account key has invalid nonce length".into()))?;
    let plaintext = cipher
        .decrypt(&nonce, ciphertext.as_ref())
        .map_err(|_| CliError::InvalidAuth("stored account key could not be decrypted".into()))?;
    String::from_utf8(plaintext)
        .map_err(|_| CliError::InvalidAuth("stored account key is not valid UTF-8".into()).into())
}

#[cfg(test)]
#[allow(dead_code)]
#[path = "../tests/support/legacy_accounts_fixture.rs"]
pub(crate) mod fixture;

#[cfg(test)]
mod tests {
    use super::fixture::{self, Row, Schema};
    use super::*;
    use tempfile::TempDir;

    const KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000007";
    const OTHER_KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000008";

    fn material() -> KeySource {
        KeySource::Material(fixture::KEY_MATERIAL.to_vec())
    }

    fn open_with_rows(rows: &[Row<'_>]) -> (TempDir, LegacyAccounts) {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("accounts.db");
        fixture::create_accounts_db(&db, &fixture::KEY_MATERIAL, Schema::Current, rows);
        let accounts = LegacyAccounts::open(&db, &material(), None).unwrap();
        (tmp, accounts)
    }

    #[test]
    fn selector_matches_id_alias_or_address_and_decrypts_the_key() {
        let (_tmp, accounts) = open_with_rows(&[
            Row::new("main", "0xabc", KEY),
            Row::new("backup", "0xdef", OTHER_KEY),
        ]);

        for selector in ["main", "0xabc", "1", " main "] {
            let account = accounts.account_by_selector(selector).unwrap().unwrap();
            assert_eq!(account.alias, "main", "selector {selector}");
            assert_eq!(accounts.decrypt_private_key(&account).unwrap(), KEY);
        }
        let backup = accounts.account_by_selector("2").unwrap().unwrap();
        assert_eq!(accounts.decrypt_private_key(&backup).unwrap(), OTHER_KEY);
        assert!(accounts.account_by_selector("missing").unwrap().is_none());
    }

    #[test]
    fn default_account_wins_over_the_first_account() {
        let (_tmp, accounts) = open_with_rows(&[
            Row::new("main", "0xabc", KEY),
            Row::new("backup", "0xdef", OTHER_KEY).default_account(),
        ]);
        assert_eq!(
            accounts.default_or_first_account().unwrap().unwrap().alias,
            "backup"
        );

        let (_tmp, accounts) = open_with_rows(&[
            Row::new("main", "0xabc", KEY),
            Row::new("backup", "0xdef", OTHER_KEY),
        ]);
        assert_eq!(
            accounts.default_or_first_account().unwrap().unwrap().alias,
            "main"
        );
    }

    #[test]
    fn agent_wallet_queries_the_master_address() {
        let master: Address = "0x0000000000000000000000000000000000000001"
            .parse()
            .unwrap();
        let agent: Address = "0x0000000000000000000000000000000000000002"
            .parse()
            .unwrap();
        let (_tmp, accounts) = open_with_rows(&[
            Row::new("agent", &agent.to_string(), KEY).agent_of(&master.to_string())
        ]);

        let account = accounts.account_by_selector("agent").unwrap().unwrap();

        assert_eq!(account.query_address().unwrap(), master);
        assert_ne!(account.query_address().unwrap(), agent);
    }

    #[test]
    fn invalid_stored_address_is_an_internal_error() {
        let (_tmp, accounts) = open_with_rows(&[Row::new("main", "0xabc", KEY)]);
        let account = accounts.account_by_selector("main").unwrap().unwrap();
        let err = account.query_address().unwrap_err();
        assert!(err.to_string().contains("invalid query address"));
    }

    #[test]
    fn databases_without_agent_columns_still_read() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("accounts.db");
        fixture::create_accounts_db(
            &db,
            &fixture::KEY_MATERIAL,
            Schema::WithoutAgentColumns,
            &[Row::new("main", "0xabc", KEY)],
        );

        let accounts = LegacyAccounts::open(&db, &material(), None).unwrap();
        let account = accounts.account_by_selector("main").unwrap().unwrap();

        assert_eq!(account.master_address, None);
        assert_eq!(accounts.decrypt_private_key(&account).unwrap(), KEY);
        let conn = Connection::open(&db).unwrap();
        assert!(!table_has_column(&conn, "master_address").unwrap());
    }

    #[test]
    fn data_directory_alone_cannot_decrypt() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("accounts.db");
        fixture::create_accounts_db(
            &db,
            &fixture::KEY_MATERIAL,
            Schema::Current,
            &[Row::new("main", "0xabc", KEY)],
        );

        let err = LegacyAccounts::open(&db, &KeySource::Unavailable, None).unwrap_err();
        assert!(err.to_string().contains("data directory alone"));
    }

    #[test]
    fn legacy_key_file_is_used_without_being_migrated_or_removed() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("data").join("accounts.db");
        let key_file = tmp.path().join("data").join("accounts.key");
        fixture::create_accounts_db_with_key_file(
            &db,
            &key_file,
            &[Row::new("main", "0xabc", KEY)],
        );
        let before = std::fs::read(&key_file).unwrap();

        let accounts = LegacyAccounts::open(&db, &KeySource::Unavailable, Some(&key_file)).unwrap();
        let account = accounts.default_or_first_account().unwrap().unwrap();

        assert_eq!(accounts.decrypt_private_key(&account).unwrap(), KEY);
        assert_eq!(std::fs::read(&key_file).unwrap(), before);
    }

    #[test]
    fn opening_is_read_only() {
        let (_tmp, accounts) = open_with_rows(&[Row::new("main", "0xabc", KEY)]);
        assert!(accounts.conn.execute("DELETE FROM accounts", []).is_err());
    }

    #[test]
    fn passphrase_key_source_rejects_empty_and_trims_line_endings() {
        assert!(KeySource::from_passphrase("\n").is_err());
        let KeySource::Material(with_newline) = KeySource::from_passphrase("secret\r\n").unwrap()
        else {
            panic!("expected key material");
        };
        let KeySource::Material(plain) = KeySource::from_passphrase("secret").unwrap() else {
            panic!("expected key material");
        };
        assert_eq!(with_newline, plain);
    }

    #[test]
    fn removed_key_store_dir_variable_is_rejected() {
        let settings = Settings::detached(
            crate::settings::EnvSource::fixed([(env::ACCOUNT_KEY_STORE_DIR, "/tmp/keys")]),
            None,
        );
        let err = KeySource::from_settings(&settings).unwrap_err();
        assert!(err.to_string().contains("no longer supported"));
    }

    #[test]
    fn decrypt_rejects_invalid_nonce_length_without_panicking() {
        let encrypted = format!(
            "{ENCRYPTION_VERSION}:{}:{}",
            BASE64.encode([1_u8; 8]),
            BASE64.encode([2_u8; 16])
        );
        let err = decrypt_private_key(&encrypted, &[7_u8; 32]).unwrap_err();
        assert!(err.to_string().contains("invalid nonce length"));
    }
}
