use crate::{info, warn};
use eyre::{Result, bail};
use std::{path::Path, process::Command};

pub(crate) fn ensure_runtime_dependencies(binary: &Path) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }

    let Some(required_library) = linked_libusb(binary) else {
        return Ok(());
    };
    if required_library.is_absolute() && required_library.is_file() {
        info("macOS runtime dependency found: libusb");
        return Ok(());
    }

    let prefix = brew_prefix()?;
    let library = prefix.join("lib/libusb-1.0.0.dylib");
    if library.is_file() {
        info("macOS runtime dependency found: libusb");
        return Ok(());
    }

    if !command_available("brew") {
        bail!(
            "macOS runtime dependency libusb is missing and Homebrew was not found. Install Homebrew, run 'brew install libusb', then re-run tempoup"
        );
    }

    warn("macOS runtime dependency libusb not found; installing with Homebrew");
    let installed = Command::new("brew")
        .args(["list", "--versions", "libusb"])
        .output()
        .is_ok_and(|output| output.status.success());
    let action = if installed { "reinstall" } else { "install" };
    let status = Command::new("brew").args([action, "libusb"]).status()?;
    if !status.success() || !library.is_file() {
        bail!("Homebrew did not install libusb where tempo can load it");
    }
    info("macOS runtime dependency installed: libusb");
    Ok(())
}

fn linked_libusb(binary: &Path) -> Option<std::path::PathBuf> {
    let output = Command::new("otool").arg("-L").arg(binary).output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_linked_libusb(&String::from_utf8_lossy(&output.stdout))
}

fn parse_linked_libusb(output: &str) -> Option<std::path::PathBuf> {
    output
        .lines()
        .find(|line| line.contains("libusb-1.0.0.dylib"))
        .and_then(|line| line.split_whitespace().next())
        .map(Into::into)
}

fn brew_prefix() -> Result<std::path::PathBuf> {
    if !command_available("brew") {
        return Ok(std::path::PathBuf::new());
    }
    let output = Command::new("brew").args(["--prefix", "libusb"]).output()?;
    if !output.status.success() {
        return Ok(std::path::PathBuf::new());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}

fn command_available(command: &str) -> bool {
    Command::new(command).arg("--version").output().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linked_libusb_path() {
        let output = "/tmp/tempo:\n\t/opt/homebrew/opt/libusb/lib/libusb-1.0.0.dylib (compatibility version 6.0.0)\n";
        assert_eq!(
            parse_linked_libusb(output),
            Some("/opt/homebrew/opt/libusb/lib/libusb-1.0.0.dylib".into())
        );
        assert_eq!(parse_linked_libusb("/usr/lib/libSystem.B.dylib"), None);
    }
}
