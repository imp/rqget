use super::Destination;
use anyhow::Result;
use rqget_test as support;
use rqget_test::metadata;

#[test]
fn new_transfer_preserves_unrelated_files() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let unrelated = root.path().join("unrelated");
    std::fs::write(&unrelated, b"unchanged")?;
    Destination::open(root.path(), false).expect_err("new transfers require an empty directory");
    assert_eq!(std::fs::read(&unrelated)?, b"unchanged");
    Ok(())
}

#[cfg(unix)]
#[test]
fn canonical_root_must_remain_utf8() -> Result<()> {
    use std::os::unix::ffi::OsStringExt;
    let root = camino_tempfile::tempdir()?;
    let raw = root
        .path()
        .as_std_path()
        .join(std::ffi::OsString::from_vec(vec![255]));
    if let Err(error) = std::fs::create_dir(&raw) {
        #[cfg(target_os = "macos")]
        {
            // Darwin EILSEQ: this filesystem rejects non-UTF-8 names at creation.
            const ILLEGAL_BYTE_SEQUENCE: i32 = 92;
            assert_eq!(error.raw_os_error(), Some(ILLEGAL_BYTE_SEQUENCE));
            return Ok(());
        }
        #[cfg(not(target_os = "macos"))]
        return Err(error.into());
    }
    let alias = root.path().join("valid-utf8-alias");
    std::os::unix::fs::symlink(&raw, &alias)?;
    Destination::open(&alias, false).expect_err("canonical paths cannot silently become non-UTF-8");
    assert_eq!(std::fs::read_dir(raw)?.count(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_rejects_missing_mismatched_and_unsupported_records() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    Destination::open(&root.path().join("missing"), true)
        .expect_err("missing record must not authorize writes");
    let destination = root.path().join("out");
    let guard = Destination::open(&destination, false)?;
    let resolved = metadata(support::metainfo(&[&[b"payload"]]), &destination).await?;
    guard.prepare(&resolved)?;
    let record_path = destination.join(".rqget/state.json");
    let original = std::fs::read(&record_path)?;
    drop(guard);
    let mut unsupported: serde_json::Value = serde_json::from_slice(&original)?;
    unsupported["version"] = serde_json::json!(2);
    std::fs::write(&record_path, serde_json::to_vec(&unsupported)?)?;
    Destination::open(&destination, true).expect_err("unsupported record version must be rejected");
    std::fs::write(&record_path, &original)?;
    let guard = Destination::open(&destination, true)?;
    let different = metadata(support::metainfo(&[&[b"other"]]), &destination).await?;
    guard
        .prepare(&different)
        .expect_err("a different torrent must not authorize writes");
    assert_eq!(std::fs::read(&record_path)?, original);
    assert_eq!(std::fs::read_dir(&destination)?.count(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_record_publication_never_creates_payload_files() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let guard = Destination::open(root.path(), false)?;
    let resolved = metadata(support::metainfo(&[&[b"payload"]]), root.path()).await?;
    std::fs::create_dir(root.path().join(".rqget/state.json"))?;
    guard
        .prepare(&resolved)
        .expect_err("record publication must succeed before payload opens");
    assert!(!root.path().join("payload").exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_rejects_hard_link_aliases() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let destination = root.path().join("out");
    let guard = Destination::open(&destination, false)?;
    let resolved = metadata(support::metainfo(&[&[b"payload"]]), &destination).await?;
    guard.prepare(&resolved)?;
    drop(guard);
    let outside = root.path().join("outside");
    std::fs::write(&outside, b"unchanged")?;
    std::fs::hard_link(&outside, destination.join("payload"))?;
    let guard = Destination::open(&destination, true)?;
    guard
        .prepare(&resolved)
        .expect_err("a payload write must not modify an outside hard-link alias");
    assert_eq!(std::fs::read(&outside)?, b"unchanged");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn destination_preflight_rejects_unsafe_metadata_without_payload_writes() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let outside = root.path().join("outside");
    std::fs::write(&outside, b"unchanged")?;
    for (index, paths) in [
        vec![vec![b".rqget".as_slice(), b"state.json"]],
        vec![vec![b"file".as_slice()], vec![b"file".as_slice(), b"child"]],
        vec![vec![b"\xff".as_slice()]],
        vec![vec![b".".as_slice()]],
    ]
    .into_iter()
    .enumerate()
    {
        let destination = root.path().join(format!("out-{index}"));
        let guard = Destination::open(&destination, false)?;
        let paths = paths.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let resolved = metadata(support::metainfo(&paths), &destination).await?;
        guard
            .prepare(&resolved)
            .expect_err("unsafe destination must be rejected");
        assert!(!destination.join(".rqget/state.json").exists());
        assert_eq!(std::fs::read_dir(&destination)?.count(), 1);
    }
    assert_eq!(std::fs::read(&outside)?, b"unchanged");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn destination_lock_records_and_resume_are_explicit() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let destination = root.path().join("out");
    let guard = Destination::open(&destination, false)?;
    Destination::open(&destination, false).expect_err("destination is already owned");
    let resolved = metadata(support::metainfo(&[&[b"payload"]]), &destination).await?;
    assert!(!guard.prepare(&resolved)?);
    drop(guard);
    Destination::open(&destination, false).expect_err("destination is already owned");
    let guard = Destination::open(&destination, true)?;
    assert!(guard.prepare(&resolved)?);
    std::fs::write(destination.join("unrelated"), b"keep me")?;
    guard
        .prepare(&resolved)
        .expect_err("unsafe destination must be rejected");
    assert_eq!(std::fs::read(destination.join("unrelated"))?, b"keep me");
    drop(guard);
    std::fs::write(destination.join(".rqget/state.json"), b"{incomplete")?;
    Destination::open(&destination, true).expect_err("invalid resume record must be rejected");
    assert_eq!(std::fs::read(destination.join("unrelated"))?, b"keep me");
    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_rejects_symlinks_and_aliases_share_the_lock() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let destination = root.path().join("out");
    let guard = Destination::open(&destination, false)?;
    let resolved = metadata(support::metainfo(&[&[b"payload"]]), &destination).await?;
    guard.prepare(&resolved)?;
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&destination, &alias)?;
    Destination::open(&alias, true).expect_err("an alias must contend for the same lock");
    drop(guard);
    let outside = root.path().join("outside");
    std::fs::write(&outside, b"x")?;
    std::os::unix::fs::symlink(&outside, destination.join("payload"))?;
    let guard = Destination::open(&alias, true)?;
    guard
        .prepare(&resolved)
        .expect_err("unsafe destination must be rejected");
    assert_eq!(std::fs::read(&outside)?, b"x");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_filename_aliases_are_rejected_before_payload_creation() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    std::fs::write(root.path().join("CaseProbe"), b"")?;
    let case_insensitive = root.path().join("caseprobe").exists();
    let destination = root.path().join("out");
    let guard = Destination::open(&destination, false)?;
    let resolved = metadata(support::metainfo(&[&[b"A"], &[b"a"]]), &destination).await?;
    assert_eq!(guard.prepare(&resolved).is_err(), case_insensitive);
    assert!(!destination.join("A").exists());
    assert!(!destination.join("a").exists());
    Ok(())
}
