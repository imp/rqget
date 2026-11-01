//! Exercise command parsing through the actual executable.

use anyhow::Result;
use std::process::Command;

#[test]
fn cli_rejects_invalid_invocations_without_destination_side_effects() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let destination = root.path().join("must-not-exist");
    for arguments in [
        vec!["--help"],
        vec!["--version"],
        vec![],
        vec!["source"],
        vec!["a", "b", "-o", destination.as_str()],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rqget"))
            .args(&arguments)
            .output()?;
        let informational = arguments == ["--help"] || arguments == ["--version"];
        assert_eq!(
            output.status.code(),
            Some(if informational { 0 } else { 2 })
        );
        assert!(!destination.exists());
    }
    for source in [
        "magnet:?xt=urn:btmh:12200123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "-",
        "0123456789abcdef0123456789abcdef0123456789",
        "https://private.example/token",
        "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef0123456789&%73%6f=0-18446744073709551615&tr=https://private.example/token",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rqget"))
            .args(["--", source])
            .output()?;
        assert!(!output.status.success());
        // Validate with otherwise valid arguments too; rejected sources never reach I/O.
        let output = Command::new(env!("CARGO_BIN_EXE_rqget"))
            .args(["-o", destination.as_str(), "--", source])
            .output()?;
        assert_eq!(output.status.code(), Some(1));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private.example"));
        assert!(!destination.exists());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn clap_rejects_non_utf8_sources() -> Result<()> {
    use std::os::unix::ffi::OsStringExt;
    let root = camino_tempfile::tempdir()?;
    let output = Command::new(env!("CARGO_BIN_EXE_rqget"))
        .arg(std::ffi::OsString::from_vec(vec![255]))
        .args(["-o", root.path().join("out").as_str()])
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(std::fs::read_dir(root.path())?.count(), 0);
    Ok(())
}
