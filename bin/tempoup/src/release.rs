use crate::{
    config::{TEMPO_REPO, TEMPOUP_REPO},
    download::Downloader,
    platform::Target,
};
use eyre::{Context, Result, bail};
use semver::Version;
use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct Asset {
    pub name: String,
}

#[derive(Deserialize)]
pub(crate) struct Release {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}

impl Release {
    pub(crate) fn require_assets(&self, names: &[&str]) -> Result<()> {
        for name in names {
            if !self.assets.iter().any(|asset| asset.name == *name) {
                bail!(
                    "release {} does not contain required asset {name}",
                    self.tag_name
                );
            }
        }
        Ok(())
    }
}

pub(crate) fn tempo_archive_name(tag: &str, target: Target) -> String {
    format!("tempo-{tag}-{}.tar.gz", target.triple())
}

pub(crate) fn tempoup_archive_name(tag: &str, target: Target) -> String {
    format!("{tag}-{}.tar.gz", target.triple())
}

pub(crate) fn tempo_release_download_url(tag: &str, asset: &str) -> String {
    release_download_url(TEMPO_REPO, tag, asset)
}

pub(crate) fn tempoup_release_download_url(tag: &str, asset: &str) -> String {
    release_download_url(TEMPOUP_REPO, tag, asset)
}

fn release_download_url(repository: &str, tag: &str, asset: &str) -> String {
    format!("https://github.com/{repository}/releases/download/{tag}/{asset}")
}

pub(crate) fn resolve_tempo_release(
    downloader: &Downloader,
    requested: Option<&str>,
) -> Result<Release> {
    match requested {
        Some(tag) => fetch_release(downloader, TEMPO_REPO, tag),
        None => latest_tempo_release(list_releases(downloader, TEMPO_REPO)?)
            .ok_or_else(|| eyre::eyre!("could not find a published Tempo release")),
    }
}

pub(crate) fn resolve_latest_tempoup_release(downloader: &Downloader) -> Result<Release> {
    latest_tempoup_release(list_releases(downloader, TEMPOUP_REPO)?)
        .ok_or_else(|| eyre::eyre!("could not find a published tempoup release"))
}

fn latest_tempoup_release(releases: Vec<Release>) -> Option<Release> {
    releases
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| tempoup_version(&release.tag_name).map(|version| (version, release)))
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, release)| release)
}

fn fetch_release(downloader: &Downloader, repository: &str, tag: &str) -> Result<Release> {
    let url = format!("https://api.github.com/repos/{repository}/releases/tags/{tag}");
    let body = downloader
        .download_to_string(&url)
        .wrap_err_with(|| format!("release {tag} was not found on GitHub"))?;
    let release: Release =
        serde_json::from_str(&body).wrap_err("invalid GitHub release response")?;
    if release.draft {
        bail!("release {tag} is still a draft and cannot be installed");
    }
    Ok(release)
}

fn list_releases(downloader: &Downloader, repository: &str) -> Result<Vec<Release>> {
    let url = format!("https://api.github.com/repos/{repository}/releases?per_page=100");
    let body = downloader.download_to_string(&url)?;
    serde_json::from_str(&body).wrap_err("invalid GitHub releases response")
}

fn is_stable_tempo_release(release: &Release) -> bool {
    !release.draft && !release.prerelease && tempo_version(&release.tag_name).is_some()
}

fn latest_tempo_release(releases: Vec<Release>) -> Option<Release> {
    releases
        .into_iter()
        .filter(is_stable_tempo_release)
        .filter_map(|release| tempo_version(&release.tag_name).map(|version| (version, release)))
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, release)| release)
}

fn tempo_version(tag: &str) -> Option<Version> {
    let raw = tag.strip_prefix('v')?;
    let version = Version::parse(raw).ok()?;
    (tag == format!("v{}.{}.{}", version.major, version.minor, version.patch)).then_some(version)
}

pub(crate) fn tempoup_version(tag: &str) -> Option<Version> {
    let version = Version::parse(tag.strip_prefix("tempoup-v")?).ok()?;
    (tag == format!(
        "tempoup-v{}.{}.{}",
        version.major, version.minor, version.patch
    ))
    .then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{Arch, Platform};

    fn release(tag: &str, draft: bool, prerelease: bool) -> Release {
        Release {
            tag_name: tag.to_string(),
            draft,
            prerelease,
            assets: Vec::new(),
        }
    }

    #[test]
    fn stable_tempo_tags_are_strict() {
        assert!(is_stable_tempo_release(&release("v1.13.2", false, false)));
        assert!(!is_stable_tempo_release(&release("v1.13.2", true, false)));
        assert!(!is_stable_tempo_release(&release("v1.13.2", false, true)));
        assert!(!is_stable_tempo_release(&release(
            "v1.13.2-rc.1",
            false,
            false
        )));
        assert!(!is_stable_tempo_release(&release(
            "tempo-primitives@1.13.2",
            false,
            false
        )));
        assert!(!is_stable_tempo_release(&release(
            "tempoup-v0.1.0",
            false,
            false
        )));
    }

    #[test]
    fn archive_names_match_release_workflows() {
        let target = Target {
            platform: Platform::Linux,
            arch: Arch::Amd64,
        };
        assert_eq!(
            tempo_archive_name("v1.13.2", target),
            "tempo-v1.13.2-x86_64-unknown-linux-gnu.tar.gz"
        );
        assert_eq!(
            tempoup_archive_name("tempoup-v0.1.0", target),
            "tempoup-v0.1.0-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    #[test]
    fn required_assets_are_checked() {
        let release = Release {
            tag_name: "v1.2.3".to_string(),
            draft: false,
            prerelease: false,
            assets: vec![Asset {
                name: "tempo.tar.gz".to_string(),
            }],
        };
        assert!(release.require_assets(&["tempo.tar.gz"]).is_ok());
        assert!(release.require_assets(&["tempo.tar.gz.sha256"]).is_err());
    }

    #[test]
    fn latest_tempoup_uses_independent_semver_tags() {
        let releases = vec![
            release("tempoup-v0.1.0", false, false),
            release("v99.0.0", false, false),
            release("tempoup-v0.3.0", true, false),
            release("tempoup-v0.4.0-rc.1", false, false),
            release("tempoup-v0.4.0+build.1", false, false),
            release("tempoup-v0.2.0", false, false),
        ];
        assert_eq!(
            latest_tempoup_release(releases).unwrap().tag_name,
            "tempoup-v0.2.0"
        );
    }

    #[test]
    fn latest_tempo_uses_semver_instead_of_api_order() {
        let releases = vec![
            release("v0.1.0", false, false),
            release("v1.13.2", false, false),
            release("v99.0.0", true, false),
            release("v2.0.0-rc.1", false, false),
        ];
        assert_eq!(latest_tempo_release(releases).unwrap().tag_name, "v1.13.2");
    }
}
