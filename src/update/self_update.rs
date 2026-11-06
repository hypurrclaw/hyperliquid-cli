//! `hyperliquid update`: download the latest release archive, verify its
//! checksum and replace the running binary.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::release::{
    InstallMethod, REPO, ReleaseClient, current_release_asset, is_newer_version, normalize_version,
};
use crate::errors::CliError;
use crate::output::{OutputFormat, TableData};
use crate::private_fs;

const TEMP_DIR_ATTEMPTS: u32 = 16;
const EXECUTABLE_MODE: u32 = 0o755;

#[derive(Debug, Clone, Serialize)]
struct UpdateResult {
    status: &'static str,
    from: String,
    to: String,
    path: Option<PathBuf>,
}

pub async fn update(format: OutputFormat, dry_run: bool) -> Result<(), anyhow::Error> {
    let current_version = env!("CARGO_PKG_VERSION");
    let client = ReleaseClient::new()?;
    let latest_version = client.latest_version().await?;
    let normalized_latest = normalize_version(&latest_version);
    let current_exe = std::env::current_exe().map_err(|err| {
        CliError::Internal(anyhow::anyhow!(
            "failed to resolve current executable: {err}"
        ))
    })?;
    let report = |status: &'static str, path: PathBuf| {
        print_update_result(
            format,
            &UpdateResult {
                status,
                from: current_version.to_string(),
                to: normalized_latest.clone(),
                path: Some(path),
            },
        );
    };

    if !is_newer_version(&latest_version, current_version) {
        report("up_to_date", current_exe);
        return Ok(());
    }

    let install_method = InstallMethod::detect(&current_exe);
    if install_method != InstallMethod::Standalone {
        return Err(CliError::Unsupported(format!(
            "Can't update {} in place: it was installed with {}.\nUpgrade:\n    {}",
            current_exe.display(),
            install_method.name(),
            install_method.upgrade_command()
        ))
        .into());
    }

    if dry_run {
        report("would_update", current_exe);
        return Ok(());
    }

    let asset = current_release_asset()?;
    let base_url = format!("https://github.com/{REPO}/releases/download/{latest_version}/{asset}");
    let archive = client.download_bytes(&base_url).await?;
    let checksum = client.download_text(&format!("{base_url}.sha256")).await?;
    verify_sha256(&archive, asset, &checksum)?;

    let tmpdir = create_temp_dir()?;
    let archive_path = tmpdir.path().join(asset);
    std::fs::write(&archive_path, archive)?;
    extract_archive(&archive_path, tmpdir.path()).await?;
    let new_binary = find_extracted_binary(tmpdir.path())?;
    replace_current_binary(&current_exe, &new_binary)?;

    report("updated", current_exe);
    Ok(())
}

fn verify_sha256(bytes: &[u8], asset: &str, checksum_text: &str) -> Result<(), CliError> {
    use sha2::{Digest, Sha256};

    let expected = checksum_text
        .lines()
        .find(|line| line.contains(asset))
        .and_then(|line| line.split_whitespace().next())
        .ok_or_else(|| {
            CliError::Unavailable(format!("release checksum did not include {asset}"))
        })?;

    let actual = hex::encode(Sha256::digest(bytes));
    if actual != expected {
        return Err(CliError::Unavailable(format!(
            "checksum mismatch for {asset}: expected {expected}, got {actual}"
        )));
    }
    Ok(())
}

async fn extract_archive(archive_path: &Path, destination: &Path) -> Result<(), anyhow::Error> {
    let status = tokio::process::Command::new("tar")
        .arg("-xzf")
        .arg(archive_path)
        .arg("-C")
        .arg(destination)
        .status()
        .await
        .map_err(|err| CliError::Unavailable(format!("failed to run tar: {err}")))?;
    if !status.success() {
        return Err(CliError::Unavailable(format!("tar exited with status {status}")).into());
    }
    Ok(())
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn create_temp_dir() -> Result<TempDir, anyhow::Error> {
    for _ in 0..TEMP_DIR_ATTEMPTS {
        let path = std::env::temp_dir().join(format!(
            "hyperliquid-update-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        match private_fs::create_new_dir(&path) {
            Ok(()) => return Ok(TempDir { path }),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.into()),
        }
    }
    Err(CliError::Internal(anyhow::anyhow!(
        "failed to create a unique temporary update directory"
    ))
    .into())
}

fn find_extracted_binary(tmpdir: &Path) -> Result<PathBuf, CliError> {
    let direct = tmpdir.join("hyperliquid");
    if direct.is_file() {
        return Ok(direct);
    }
    find_extracted_binary_recursive(tmpdir)?.ok_or_else(|| {
        CliError::Unavailable(
            "release archive did not contain executable 'hyperliquid'".to_string(),
        )
    })
}

fn find_extracted_binary_recursive(dir: &Path) -> Result<Option<PathBuf>, CliError> {
    let inspect_failed = |err: std::io::Error| {
        CliError::Internal(anyhow::anyhow!("failed to inspect release archive: {err}"))
    };
    for entry in std::fs::read_dir(dir).map_err(inspect_failed)? {
        let path = entry.map_err(inspect_failed)?.path();
        if path.file_name().is_some_and(|name| name == "hyperliquid") && path.is_file() {
            return Ok(Some(path));
        }
        if path.is_dir()
            && let Some(binary) = find_extracted_binary_recursive(&path)?
        {
            return Ok(Some(binary));
        }
    }
    Ok(None)
}

/// Copy `source` to `destination` as a new executable file. `create_new`
/// (`O_EXCL`) refuses to follow a pre-planted file or symlink at the target.
fn copy_to_new_executable(source: &Path, destination: &Path) -> std::io::Result<()> {
    let mut input = std::fs::File::open(source)?;
    let mut output = private_fs::open_options_with_mode(EXECUTABLE_MODE)
        .write(true)
        .create_new(true)
        .open(destination)?;
    std::io::copy(&mut input, &mut output)?;
    output.sync_all()
}

fn replace_current_binary(current_exe: &Path, new_binary: &Path) -> Result<(), anyhow::Error> {
    let parent = current_exe.parent().ok_or_else(|| {
        CliError::Internal(anyhow::anyhow!(
            "current executable has no parent directory"
        ))
    })?;
    let tmp_path = parent.join(format!(
        ".hyperliquid-update-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    if let Err(err) = copy_to_new_executable(new_binary, &tmp_path) {
        if err.kind() != std::io::ErrorKind::AlreadyExists {
            let _ = std::fs::remove_file(&tmp_path);
        }
        return Err(CliError::Unavailable(format!(
            "failed to write updated binary next to {}: {err}",
            current_exe.display()
        ))
        .into());
    }
    std::fs::rename(&tmp_path, current_exe).map_err(|err| {
        let _ = std::fs::remove_file(&tmp_path);
        CliError::Unavailable(format!(
            "failed to replace {}: {err}",
            current_exe.display()
        ))
    })?;
    Ok(())
}

impl TableData for UpdateResult {
    fn headers(&self) -> Vec<&str> {
        vec!["Status", "From", "To", "Path"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.status.to_string(),
            self.from.clone(),
            self.to.clone(),
            self.path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

fn print_update_result(format: OutputFormat, result: &UpdateResult) {
    match format {
        OutputFormat::Json | OutputFormat::Table => {
            crate::output::print_data_no_timing(result, format);
        }
        OutputFormat::Pretty => match result.status {
            "updated" => println!("Updated hyperliquid {} -> {}", result.from, result.to),
            "would_update" => println!("Would update hyperliquid {} -> {}", result.from, result.to),
            "up_to_date" => println!("hyperliquid is up to date ({})", result.from),
            status => println!("hyperliquid update status: {status}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_requires_target_asset_line() {
        let checksum = format!("{}  other-asset.tar.gz", "0".repeat(64));
        let err = verify_sha256(b"archive", "hyperliquid-target.tar.gz", &checksum).unwrap_err();
        assert!(
            err.to_string()
                .contains("release checksum did not include hyperliquid-target.tar.gz")
        );
    }

    #[test]
    fn checksum_mismatch_names_both_digests() {
        let checksum = format!("{}  hyperliquid-target.tar.gz", "0".repeat(64));
        let err = verify_sha256(b"archive", "hyperliquid-target.tar.gz", &checksum).unwrap_err();
        assert!(err.to_string().contains("checksum mismatch"));
    }

    #[test]
    fn extracted_binary_searches_nested_directories() {
        let tmpdir = create_temp_dir().unwrap();
        let nested = tmpdir.path().join("release").join("bin");
        std::fs::create_dir_all(&nested).unwrap();
        let binary = nested.join("hyperliquid");
        std::fs::write(&binary, b"binary").unwrap();

        assert_eq!(find_extracted_binary(tmpdir.path()).unwrap(), binary);
    }

    #[test]
    fn temp_dir_uses_randomized_update_prefix() {
        let first = create_temp_dir().unwrap();
        let second = create_temp_dir().unwrap();
        let name = first.path().file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("hyperliquid-update-"));
        assert_ne!(first.path(), second.path());
    }

    #[cfg(unix)]
    #[test]
    fn temp_dir_is_owner_only_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let tmpdir = create_temp_dir().unwrap();
        let mode = std::fs::metadata(tmpdir.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn update_binary_is_written_with_create_new() {
        let tmpdir = create_temp_dir().unwrap();
        let source = tmpdir.path().join("source");
        std::fs::write(&source, b"binary").unwrap();
        let destination = tmpdir.path().join("destination");

        copy_to_new_executable(&source, &destination).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&destination)
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode & 0o700, 0o700);
        }

        let err = copy_to_new_executable(&source, &destination).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&destination).unwrap(), b"binary");
    }
}
