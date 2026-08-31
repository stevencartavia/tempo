use std::process::Command;

#[test]
fn version_and_help_match_the_public_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_tempoup"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0.1.0");
    let output = Command::new(env!("CARGO_BIN_EXE_tempoup"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for flag in ["--install", "--update", "--unsafe-skip-verify", "--version"] {
        assert!(stdout.contains(flag), "missing {flag} in:\n{stdout}");
    }
}
