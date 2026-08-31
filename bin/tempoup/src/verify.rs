use crate::{
    config::GPG_KEY_FINGERPRINT,
    download::{Downloader, compute_sha256},
    info, warn,
};
use eyre::{Result, bail};
use semver::Version;
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

const GPG_KEY_URL: &str = "https://keyserver.ubuntu.com/pks/lookup?op=get&search=0xEE3C5D41EA963E896F310EC3CBBFA54B20D33446";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerificationMethod {
    Unsafe,
    GitHubAttestation,
    Gpg,
    LegacyChecksumOnly,
}

pub(crate) fn select_method(
    tag: &str,
    unsafe_skip_verify: bool,
    allow_legacy: bool,
) -> Result<VerificationMethod> {
    if unsafe_skip_verify {
        warn("skipping release integrity verification (--unsafe-skip-verify)");
        return Ok(VerificationMethod::Unsafe);
    }
    method_for(
        tag,
        false,
        allow_legacy,
        gh_authenticated(),
        command_available("gpg"),
    )
}

fn method_for(
    tag: &str,
    unsafe_skip_verify: bool,
    allow_legacy: bool,
    gh_authenticated: bool,
    gpg_available: bool,
) -> Result<VerificationMethod> {
    if unsafe_skip_verify {
        Ok(VerificationMethod::Unsafe)
    } else if gh_authenticated {
        Ok(VerificationMethod::GitHubAttestation)
    } else if gpg_available && allow_legacy && !requires_gpg(tag)? {
        Ok(VerificationMethod::LegacyChecksumOnly)
    } else if gpg_available {
        Ok(VerificationMethod::Gpg)
    } else {
        bail!(
            "gh is unavailable or unauthenticated, and gpg was not found. Run 'gh auth login', install gpg, or re-run with --unsafe-skip-verify"
        )
    }
}

pub(crate) fn verify_checksum(archive: &Path, checksum_file: &Path) -> Result<()> {
    let contents = fs::read_to_string(checksum_file)?;
    let expected = contents
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| eyre::eyre!("invalid SHA-256 checksum file"))?;
    let actual = compute_sha256(archive)?;
    if !expected.eq_ignore_ascii_case(&actual) {
        bail!("checksum verification failed\n  Expected: {expected}\n  Actual:   {actual}");
    }
    info("checksum verified ✓");
    Ok(())
}

pub(crate) fn verify_artifact(
    downloader: &Downloader,
    archive: &Path,
    signature: Option<&Path>,
    method: VerificationMethod,
    repository: &str,
) -> Result<()> {
    match method {
        VerificationMethod::Unsafe => Ok(()),
        VerificationMethod::LegacyChecksumOnly => {
            info("skipping GPG verification for legacy release");
            Ok(())
        }
        VerificationMethod::GitHubAttestation => {
            info("verifying release attestation with gh");
            let status = Command::new("gh")
                .args([
                    "attestation",
                    "verify",
                    archive.to_string_lossy().as_ref(),
                    "--repo",
                    repository,
                    "--predicate-type",
                    "https://slsa.dev/provenance/v1",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            if !status.success() {
                bail!("release attestation not found or failed verification");
            }
            info("release attestation verified ✓");
            Ok(())
        }
        VerificationMethod::Gpg => {
            let signature = signature.ok_or_else(|| eyre::eyre!("missing GPG signature"))?;
            ensure_gpg_key(downloader)?;
            info("verifying GPG signature");
            let status = Command::new("gpg")
                .args(["--batch", "--verify"])
                .arg(signature)
                .arg(archive)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            if !status.success() {
                bail!("GPG signature verification failed");
            }
            info("GPG signature verified ✓");
            Ok(())
        }
    }
}

fn ensure_gpg_key(downloader: &Downloader) -> Result<()> {
    if gpg_has_release_key() {
        return Ok(());
    }

    info("fetching Tempo release signing key");
    let directory = tempfile::tempdir()?;
    let key_path = directory.path().join("tempo-release-key.asc");
    downloader.download_to_file(GPG_KEY_URL, &key_path)?;
    let status = Command::new("gpg")
        .arg("--batch")
        .arg("--import")
        .arg(&key_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() || !gpg_has_release_key() {
        bail!("failed to import Tempo release signing key");
    }
    Ok(())
}

fn gpg_has_release_key() -> bool {
    Command::new("gpg")
        .args(["--batch", "--list-keys", GPG_KEY_FINGERPRINT])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn gh_authenticated() -> bool {
    Command::new("gh")
        .args(["auth", "status"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn command_available(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn requires_gpg(tag: &str) -> Result<bool> {
    let version = Version::parse(tag.trim_start_matches('v'))?;
    Ok(version > Version::new(1, 1, 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_rejects_checksums() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("archive");
        let checksum = directory.path().join("archive.sha256");
        fs::write(&archive, b"tempo").unwrap();
        fs::write(
            &checksum,
            "8d6546721a1d106cf8d27f7326ebae7e83c1592aeb7479b8f7ec9d8d700d464f  archive\n",
        )
        .unwrap();
        verify_checksum(&archive, &checksum).unwrap();
        fs::write(&checksum, format!("{}  archive\n", "0".repeat(64))).unwrap();
        assert!(verify_checksum(&archive, &checksum).is_err());
        fs::write(&checksum, "not-a-checksum\n").unwrap();
        assert!(verify_checksum(&archive, &checksum).is_err());
    }

    #[test]
    fn preserves_legacy_gpg_cutoff() {
        assert!(!requires_gpg("v1.1.2").unwrap());
        assert!(requires_gpg("v1.1.3").unwrap());
    }

    #[test]
    fn verification_method_matches_bash_precedence() {
        assert_eq!(
            method_for("v2.0.0", true, true, false, false).unwrap(),
            VerificationMethod::Unsafe
        );
        assert_eq!(
            method_for("v1.0.0", false, true, true, true).unwrap(),
            VerificationMethod::GitHubAttestation
        );
        assert_eq!(
            method_for("v1.1.2", false, true, false, true).unwrap(),
            VerificationMethod::LegacyChecksumOnly
        );
        assert_eq!(
            method_for("v1.1.3", false, true, false, true).unwrap(),
            VerificationMethod::Gpg
        );
        assert!(method_for("v2.0.0", false, true, false, false).is_err());
    }
}
