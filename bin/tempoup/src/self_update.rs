use crate::{
    config::{Config, TEMPOUP_REPO, VERSION},
    download::{Downloader, extract_tar_gz_file},
    info,
    platform::{Target, set_executable},
    release::{
        resolve_latest_tempoup_release, tempoup_archive_name, tempoup_release_download_url,
        tempoup_version,
    },
    verify::{VerificationMethod, select_method, verify_artifact, verify_checksum},
};
use eyre::{Context, Result, bail};
use semver::Version;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) fn check_for_update() -> Result<Option<Version>> {
    let downloader = Downloader::new()?;
    let release = resolve_latest_tempoup_release(&downloader)?;
    let remote = tempoup_version(&release.tag_name)
        .ok_or_else(|| eyre::eyre!("invalid tempoup release tag {}", release.tag_name))?;
    Ok((remote > Version::parse(VERSION)?).then_some(remote))
}

pub(crate) fn run(config: &Config, unsafe_skip_verify: bool) -> Result<()> {
    info("checking for tempoup updates");
    fs::create_dir_all(&config.bin_dir)?;
    let downloader = Downloader::new()?;
    let release = resolve_latest_tempoup_release(&downloader)?;
    let remote = tempoup_version(&release.tag_name)
        .ok_or_else(|| eyre::eyre!("invalid tempoup release tag {}", release.tag_name))?;
    let current = Version::parse(VERSION)?;
    if remote <= current {
        info(format!("tempoup is already up to date (version {VERSION})"));
        return Ok(());
    }

    let target = Target::detect()?;
    let archive_name = tempoup_archive_name(&release.tag_name, target);
    let checksum_name = format!("{archive_name}.sha256");
    let signature_name = format!("{archive_name}.asc");
    let method = select_method(&release.tag_name, unsafe_skip_verify, false)?;
    let mut required = vec![archive_name.as_str(), checksum_name.as_str()];
    if method == VerificationMethod::Gpg {
        required.push(signature_name.as_str());
    }
    release.require_assets(&required)?;

    let workspace = tempfile::Builder::new()
        .prefix(".tempoup-self-update-")
        .tempdir_in(&config.bin_dir)?;
    let archive_path = workspace.path().join(&archive_name);
    let checksum_path = workspace.path().join(&checksum_name);
    downloader.download_to_file(
        &tempoup_release_download_url(&release.tag_name, &archive_name),
        &archive_path,
    )?;
    downloader.download_to_file(
        &tempoup_release_download_url(&release.tag_name, &checksum_name),
        &checksum_path,
    )?;
    verify_checksum(&archive_path, &checksum_path)?;

    let signature_path = if method == VerificationMethod::Gpg {
        let path = workspace.path().join(&signature_name);
        downloader.download_to_file(
            &tempoup_release_download_url(&release.tag_name, &signature_name),
            &path,
        )?;
        Some(path)
    } else {
        None
    };
    verify_artifact(
        &downloader,
        &archive_path,
        signature_path.as_deref(),
        method,
        TEMPOUP_REPO,
    )?;

    let binary_name = archive_name
        .strip_suffix(".tar.gz")
        .ok_or_else(|| eyre::eyre!("invalid tempoup archive name"))?;
    let extracted = workspace.path().join("extracted");
    let replacement = prepare_replacement(&archive_path, &extracted, binary_name, &remote)?;

    self_replace::self_replace(&replacement).wrap_err("failed to replace tempoup binary")?;
    info(format!(
        "successfully updated tempoup: {VERSION} → {remote}"
    ));
    Ok(())
}

fn prepare_replacement(
    archive_path: &Path,
    extraction_dir: &Path,
    binary_name: &str,
    expected_version: &Version,
) -> Result<PathBuf> {
    extract_tar_gz_file(archive_path, extraction_dir, binary_name)?;
    let replacement = extraction_dir.join(binary_name);
    if !replacement.is_file() {
        bail!("tempoup release archive did not contain {binary_name}");
    }
    set_executable(&replacement)?;

    let output = Command::new(&replacement).arg("--version").output()?;
    if !output.status.success() {
        bail!("downloaded tempoup binary failed its version check");
    }
    let reported = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if Version::parse(&reported)? != *expected_version {
        bail!("downloaded tempoup reported version {reported}, expected {expected_version}");
    }
    Ok(replacement)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn replacement_archive(path: &Path, binary_name: &str, version: &str) {
        use std::io::Write;

        let encoder = flate2::write::GzEncoder::new(
            fs::File::create(path).unwrap(),
            flate2::Compression::default(),
        );
        let mut archive = tar::Builder::new(encoder);
        let script = format!("#!/bin/sh\nprintf '%s\\n' '{version}'\n");
        let mut header = tar::Header::new_gnu();
        header.set_size(script.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, binary_name, script.as_bytes())
            .unwrap();
        archive
            .into_inner()
            .unwrap()
            .finish()
            .unwrap()
            .flush()
            .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn prepares_and_checks_local_replacement_archive() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("tempoup.tar.gz");
        let binary_name = "tempoup-v0.2.0-aarch64-unknown-linux-gnu";
        replacement_archive(&archive, binary_name, "0.2.0");

        let replacement = prepare_replacement(
            &archive,
            &directory.path().join("valid"),
            binary_name,
            &Version::new(0, 2, 0),
        )
        .unwrap();
        assert!(replacement.is_file());
        assert!(
            prepare_replacement(
                &archive,
                &directory.path().join("wrong-version"),
                binary_name,
                &Version::new(0, 3, 0),
            )
            .is_err()
        );
    }
}
