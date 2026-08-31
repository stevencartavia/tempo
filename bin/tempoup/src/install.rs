use crate::{
    config::{Config, TEMPO_REPO},
    download::{Downloader, extract_tar_gz_file},
    info,
    macos::ensure_runtime_dependencies,
    platform::{Target, set_executable},
    release::{resolve_tempo_release, tempo_archive_name, tempo_release_download_url},
    verify::{VerificationMethod, select_method, verify_artifact, verify_checksum},
    warn,
};
use eyre::{Context, Result, bail};
use std::{fs, path::Path, process::Command};

pub(crate) fn run(
    config: &Config,
    requested_version: Option<&str>,
    unsafe_skip_verify: bool,
) -> Result<()> {
    info("installing tempo");
    fs::create_dir_all(&config.bin_dir)?;

    let target = Target::detect()?;
    info(format!(
        "detected platform: {} ({})",
        target.platform, target.arch
    ));

    let downloader = Downloader::new()?;
    let release = resolve_tempo_release(&downloader, requested_version)?;
    let tag = release.tag_name.as_str();
    let archive_name = tempo_archive_name(tag, target);
    let checksum_name = format!("{archive_name}.sha256");
    let method = select_method(tag, unsafe_skip_verify, true)?;
    let signature_name = format!("{archive_name}.asc");

    let mut required = vec![archive_name.as_str(), checksum_name.as_str()];
    if method == VerificationMethod::Gpg {
        required.push(signature_name.as_str());
    }
    release.require_assets(&required)?;

    info(format!("installing tempo {tag}"));
    let workspace = tempfile::Builder::new()
        .prefix(".tempoup-")
        .tempdir_in(&config.bin_dir)?;
    let archive_path = workspace.path().join(&archive_name);
    let checksum_path = workspace.path().join(&checksum_name);

    downloader.download_to_file(
        &tempo_release_download_url(tag, &archive_name),
        &archive_path,
    )?;
    downloader.download_to_file(
        &tempo_release_download_url(tag, &checksum_name),
        &checksum_path,
    )?;
    verify_checksum(&archive_path, &checksum_path)?;

    let signature_path = if method == VerificationMethod::Gpg {
        let path = workspace.path().join(&signature_name);
        downloader.download_to_file(&tempo_release_download_url(tag, &signature_name), &path)?;
        Some(path)
    } else {
        None
    };
    verify_artifact(
        &downloader,
        &archive_path,
        signature_path.as_deref(),
        method,
        TEMPO_REPO,
    )?;

    let binary_name = archive_name
        .strip_suffix(".tar.gz")
        .ok_or_else(|| eyre::eyre!("invalid Tempo archive name"))?;
    let extracted = workspace.path().join("extracted");
    extract_tar_gz_file(&archive_path, &extracted, binary_name)?;
    let extracted_binary = extracted.join(binary_name);
    if !extracted_binary.is_file() {
        bail!("could not find tempo binary in downloaded archive");
    }

    set_executable(&extracted_binary)?;
    ensure_runtime_dependencies(&extracted_binary)?;
    let staged = workspace.path().join("tempo-new");
    fs::copy(&extracted_binary, &staged)?;
    set_executable(&staged)?;
    verify_tempo_binary(&staged)?;
    activate(&staged, &config.tempo_path())?;

    info(format!("✓ Tempo {tag} installed successfully!"));
    if !path_contains(&config.bin_dir) {
        warn(format!("{} is not in your PATH", config.bin_dir.display()));
    }
    if let Ok(Some(version)) = crate::self_update::check_for_update() {
        warn(format!(
            "tempoup {version} is available; run 'tempoup --update' to install it"
        ));
    }
    Ok(())
}

fn activate(staged: &Path, destination: &Path) -> Result<()> {
    let backup = destination.with_extension("old");
    recover_stale_backup(destination, &backup)?;
    let had_previous = destination.exists();

    if had_previous {
        fs::rename(destination, &backup).wrap_err("failed to back up existing tempo binary")?;
    }

    if let Err(error) = fs::rename(staged, destination) {
        if had_previous {
            let _ = fs::rename(&backup, destination);
        }
        return Err(error).wrap_err("failed to install tempo binary");
    }

    if let Err(error) = verify_tempo_binary(destination) {
        let _ = fs::remove_file(destination);
        if had_previous {
            let _ = fs::rename(&backup, destination);
        }
        return Err(error).wrap_err("new tempo binary failed after activation");
    }

    if had_previous && fs::remove_file(&backup).is_err() {
        warn(format!(
            "could not remove backup at {}; it is safe to remove manually",
            backup.display()
        ));
    }
    Ok(())
}

fn recover_stale_backup(destination: &Path, backup: &Path) -> Result<()> {
    if !backup.exists() {
        return Ok(());
    }
    if destination.exists() {
        fs::remove_file(backup).wrap_err("failed to remove stale tempo backup")?;
    } else {
        fs::rename(backup, destination).wrap_err("failed to restore interrupted tempo update")?;
    }
    Ok(())
}

fn verify_tempo_binary(path: &Path) -> Result<String> {
    let output = Command::new(path).arg("--version").output()?;
    if !output.status.success() {
        bail!(
            "downloaded tempo binary could not launch: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let version = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    info(format!("verified tempo launches: {version}"));
    Ok(version)
}

fn path_contains(directory: &Path) -> bool {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .any(|path| path == directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn executable(path: &Path, body: &str) {
        fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        set_executable(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn activation_is_transactional_and_recovers_stale_backups() {
        {
            let directory = tempfile::tempdir().unwrap();
            let destination = directory.path().join("tempo");
            let staged = directory.path().join("staged");
            executable(&destination, "echo old");
            executable(&staged, "echo new");
            activate(&staged, &destination).unwrap();
            assert_eq!(verify_tempo_binary(&destination).unwrap(), "new");
            assert!(!destination.with_extension("old").exists());
        }
        {
            let directory = tempfile::tempdir().unwrap();
            let destination = directory.path().join("tempo");
            let staged = directory.path().join("staged");
            executable(&destination, "echo old");
            executable(&staged, "exit 1");
            assert!(activate(&staged, &destination).is_err());
            assert_eq!(verify_tempo_binary(&destination).unwrap(), "old");
            assert!(!destination.with_extension("old").exists());
        }
        {
            let directory = tempfile::tempdir().unwrap();
            let destination = directory.path().join("tempo");
            let backup = destination.with_extension("old");
            executable(&backup, "echo restored");
            recover_stale_backup(&destination, &backup).unwrap();
            assert_eq!(verify_tempo_binary(&destination).unwrap(), "restored");
            assert!(!backup.exists());
        }
    }
}
