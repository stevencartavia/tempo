use eyre::{Result, eyre};
use std::path::PathBuf;

pub(crate) const TEMPO_REPO: &str = "tempoxyz/tempo";
// tempoup releases remain in the Tempo repository until the standalone repository exists.
pub(crate) const TEMPOUP_REPO: &str = "tempoxyz/tempo";
pub(crate) const GPG_KEY_FINGERPRINT: &str = "EE3C5D41EA963E896F310EC3CBBFA54B20D33446";
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub(crate) struct Config {
    pub bin_dir: PathBuf,
}

impl Config {
    pub(crate) fn from_env() -> Result<Self> {
        let bin_dir = resolve_bin_dir(
            env_path("TEMPO_BIN_DIR"),
            env_path("TEMPO_DIR"),
            dirs_next::home_dir(),
        )?;
        Ok(Self { bin_dir })
    }

    pub(crate) fn tempo_path(&self) -> PathBuf {
        self.bin_dir
            .join(if cfg!(windows) { "tempo.exe" } else { "tempo" })
    }
}

fn resolve_bin_dir(
    tempo_bin_dir: Option<PathBuf>,
    tempo_dir: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf> {
    if let Some(bin_dir) = tempo_bin_dir {
        return Ok(bin_dir);
    }
    if let Some(tempo_dir) = tempo_dir {
        return Ok(tempo_dir.join("bin"));
    }
    Ok(home
        .ok_or_else(|| eyre!("could not determine home directory"))?
        .join(".tempo/bin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_dir_precedence_matches_documented_environment() {
        let home = PathBuf::from("/home/tempo");
        assert_eq!(
            resolve_bin_dir(None, None, Some(home.clone())).unwrap(),
            home.join(".tempo/bin")
        );
        assert_eq!(
            resolve_bin_dir(None, Some(PathBuf::from("/tempo")), None).unwrap(),
            PathBuf::from("/tempo/bin")
        );
        assert_eq!(
            resolve_bin_dir(
                Some(PathBuf::from("/binaries")),
                Some(PathBuf::from("/tempo")),
                None,
            )
            .unwrap(),
            PathBuf::from("/binaries")
        );
        assert!(resolve_bin_dir(None, None, None).is_err());
    }
}
