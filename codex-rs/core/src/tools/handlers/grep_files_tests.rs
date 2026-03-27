use super::*;
use std::process::Command as StdCommand;
use tempfile::tempdir;

#[tokio::test]
async fn run_search_returns_results() -> anyhow::Result<()> {
    if !rg_available() {
        return Ok(());
    }
    let temp = tempdir().expect("create temp dir");
    let dir = temp.path();
    std::fs::write(dir.join("match_one.txt"), "alpha beta gamma").unwrap();
    std::fs::write(dir.join("match_two.txt"), "alpha delta").unwrap();
    std::fs::write(dir.join("other.txt"), "omega").unwrap();

    let results = run_rg_search("alpha", None, dir, 10, dir, false).await?;
    assert_eq!(results.len(), 2);
    assert!(results.iter().any(|path| path.ends_with("match_one.txt")));
    assert!(results.iter().any(|path| path.ends_with("match_two.txt")));
    Ok(())
}

#[tokio::test]
async fn run_search_with_glob_filter() -> anyhow::Result<()> {
    if !rg_available() {
        return Ok(());
    }
    let temp = tempdir().expect("create temp dir");
    let dir = temp.path();
    std::fs::write(dir.join("match_one.rs"), "alpha beta gamma").unwrap();
    std::fs::write(dir.join("match_two.txt"), "alpha delta").unwrap();

    let results = run_rg_search("alpha", Some("*.rs"), dir, 10, dir, false).await?;
    assert_eq!(results.len(), 1);
    assert!(results.iter().all(|path| path.ends_with("match_one.rs")));
    Ok(())
}

#[tokio::test]
async fn run_search_respects_limit() -> anyhow::Result<()> {
    if !rg_available() {
        return Ok(());
    }
    let temp = tempdir().expect("create temp dir");
    let dir = temp.path();
    std::fs::write(dir.join("one.txt"), "alpha one").unwrap();
    std::fs::write(dir.join("two.txt"), "alpha two").unwrap();
    std::fs::write(dir.join("three.txt"), "alpha three").unwrap();

    let results = run_rg_search("alpha", None, dir, 2, dir, false).await?;
    assert_eq!(results.len(), 2);
    Ok(())
}

#[tokio::test]
async fn run_search_handles_no_matches() -> anyhow::Result<()> {
    if !rg_available() {
        return Ok(());
    }
    let temp = tempdir().expect("create temp dir");
    let dir = temp.path();
    std::fs::write(dir.join("one.txt"), "omega").unwrap();

    let results = run_rg_search("alpha", None, dir, 5, dir, false).await?;
    assert!(results.is_empty());
    Ok(())
}

fn rg_available() -> bool {
    StdCommand::new("rg")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
