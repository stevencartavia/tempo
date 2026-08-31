use eyre::{Result, bail};
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    Linux,
    Darwin,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arch {
    Amd64,
    Arm64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Target {
    pub platform: Platform,
    pub arch: Arch,
}

impl Target {
    pub(crate) fn detect() -> Result<Self> {
        let platform = if cfg!(target_os = "linux") {
            Platform::Linux
        } else if cfg!(target_os = "macos") {
            Platform::Darwin
        } else {
            bail!("unsupported platform: {}", std::env::consts::OS);
        };

        let arch = match std::env::consts::ARCH {
            "x86_64" if is_rosetta() => Arch::Arm64,
            "x86_64" => Arch::Amd64,
            "aarch64" => Arch::Arm64,
            arch => bail!("unsupported architecture: {arch}"),
        };

        let target = Self { platform, arch };
        target.ensure_supported()?;
        Ok(target)
    }

    fn ensure_supported(self) -> Result<()> {
        if self.platform == Platform::Darwin && self.arch != Arch::Arm64 {
            bail!("unsupported Darwin architecture: {arch}", arch = self.arch);
        }
        Ok(())
    }

    pub(crate) fn triple(self) -> &'static str {
        match (self.platform, self.arch) {
            (Platform::Linux, Arch::Amd64) => "x86_64-unknown-linux-gnu",
            (Platform::Linux, Arch::Arm64) => "aarch64-unknown-linux-gnu",
            (Platform::Darwin, Arch::Arm64) => "aarch64-apple-darwin",
            (Platform::Darwin, Arch::Amd64) => unreachable!("validated by ensure_supported"),
        }
    }
}

pub(crate) fn set_executable(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Linux => "linux",
            Self::Darwin => "darwin",
        })
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Amd64 => "amd64",
            Self::Arm64 => "arm64",
        })
    }
}

fn is_rosetta() -> bool {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sysctl")
            .args(["-n", "sysctl.proc_translated"])
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim() == "1")
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_targets_match_release_assets() {
        assert_eq!(
            Target {
                platform: Platform::Linux,
                arch: Arch::Amd64
            }
            .triple(),
            "x86_64-unknown-linux-gnu"
        );
        assert_eq!(
            Target {
                platform: Platform::Linux,
                arch: Arch::Arm64
            }
            .triple(),
            "aarch64-unknown-linux-gnu"
        );
        assert_eq!(
            Target {
                platform: Platform::Darwin,
                arch: Arch::Arm64
            }
            .triple(),
            "aarch64-apple-darwin"
        );
        let target = Target {
            platform: Platform::Darwin,
            arch: Arch::Amd64,
        };
        assert!(target.ensure_supported().is_err());
    }
}
