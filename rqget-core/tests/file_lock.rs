use std::{
    fs::{File, OpenOptions, TryLockError},
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use anyhow::{Context, Result};
use camino::Utf8Path;

fn open_lock(path: &Utf8Path) -> Result<File> {
    Ok(OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?)
}

// Subprocess entry point: hold the kernel lock until the parent kills this process.
#[test]
fn lock_holder() -> Result<()> {
    let Some(path) = std::env::var_os("RQGET_TEST_LOCK_PATH") else {
        return Ok(());
    };
    let path = camino::Utf8PathBuf::from_path_buf(path.into())
        .map_err(|_path| anyhow::anyhow!("non-UTF-8 lock test path"))?;
    let file = open_lock(&path)?;
    file.try_lock()?;
    println!("RQGET_LOCK_READY");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    drop(file);
    Ok(())
}

#[test]
fn exclusive_lock_blocks_another_process_and_releases_after_kill() -> Result<()> {
    let directory = camino_tempfile::tempdir()?;
    let path = directory.path().join("lock");
    let mut child = Command::new(std::env::current_exe()?)
        .args(["--exact", "lock_holder", "--nocapture"])
        .env("RQGET_TEST_LOCK_PATH", &path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = (|| {
        let mut output = BufReader::new(child.stdout.take().context("missing child stdout")?);
        loop {
            let mut line = String::new();
            anyhow::ensure!(
                output.read_line(&mut line)? != 0,
                "lock holder exited before ready"
            );
            if line.contains("RQGET_LOCK_READY") {
                break;
            }
        }
        let competitor = open_lock(&path)?;
        assert!(matches!(
            competitor.try_lock(),
            Err(TryLockError::WouldBlock)
        ));

        #[cfg(unix)]
        {
            let alias = directory.path().join("alias");
            std::os::unix::fs::symlink(&path, &alias)?;
            let aliased = open_lock(&alias)?;
            assert!(matches!(aliased.try_lock(), Err(TryLockError::WouldBlock)));
        }
        Ok(())
    })();
    // Always reap the subprocess, including an unsuccessful readiness/lock check.
    let killed = child.kill();
    let reaped = child.wait();
    result?;
    killed?;
    assert!(!reaped?.success());
    let next_owner = open_lock(&path)?;
    next_owner.try_lock()?;
    assert!(
        path.exists(),
        "lock ownership must not require deleting the lock inode"
    );
    Ok(())
}
