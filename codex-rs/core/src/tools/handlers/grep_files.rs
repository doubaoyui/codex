use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use grep_regex::RegexMatcher;
use grep_searcher::sinks::UTF8;
use grep_searcher::Searcher;
use ignore::WalkBuilder;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

pub struct GrepFilesHandler;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 2000;
const SEARCH_TIMEOUT: Duration = Duration::from_secs(30);

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

#[derive(Deserialize)]
struct GrepFilesArgs {
    pattern: String,
    #[serde(default)]
    include: Option<String>,
    #[serde(default)]
    include_hidden: bool,
    #[serde(default)]
    path: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[async_trait]
impl ToolHandler for GrepFilesHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation { payload, turn, .. } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "grep_files handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: GrepFilesArgs = serde_json::from_str(&arguments).map_err(|err| {
            FunctionCallError::RespondToModel(format!(
                "failed to parse function arguments: {err:?}"
            ))
        })?;

        let pattern = args.pattern.trim();
        if pattern.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "pattern must not be empty".to_string(),
            ));
        }

        if args.limit == 0 {
            return Err(FunctionCallError::RespondToModel(
                "limit must be greater than zero".to_string(),
            ));
        }

        let limit = args.limit.min(MAX_LIMIT);
        let search_path = turn.resolve_path(args.path.clone());
        let include_hidden = args.include_hidden;

        verify_path_exists(&search_path).await?;

        let include = args.include.as_deref().map(str::trim).and_then(|val| {
            if val.is_empty() {
                None
            } else {
                Some(val.to_string())
            }
        });

        let search_results = run_rg_search(
            pattern,
            include.as_deref(),
            include_hidden,
            &search_path,
            limit,
            &turn.cwd,
        )
        .await?;

        if search_results.is_empty() {
            Ok(ToolOutput::Function {
                content: "No matches found.".to_string(),
                content_items: None,
                success: Some(false),
            })
        } else {
            Ok(ToolOutput::Function {
                content: search_results.join("\n"),
                content_items: None,
                success: Some(true),
            })
        }
    }
}

async fn verify_path_exists(path: &Path) -> Result<(), FunctionCallError> {
    tokio::fs::metadata(path).await.map_err(|err| {
        FunctionCallError::RespondToModel(format!("unable to access `{}`: {err}", path.display()))
    })?;
    Ok(())
}

async fn run_rg_search(
    pattern: &str,
    include: Option<&str>,
    include_hidden: bool,
    search_path: &Path,
    limit: usize,
    _cwd: &Path,
) -> Result<Vec<String>, FunctionCallError> {
    // Create regex matcher
    let matcher = RegexMatcher::new(pattern).map_err(|err| {
        FunctionCallError::RespondToModel(format!("Invalid regex pattern: {err}"))
    })?;

    // Create glob matcher if include pattern is provided
    let glob_matcher: Option<GlobMatcher> = if let Some(glob_pattern) = include {
        let glob = Glob::new(glob_pattern).map_err(|err| {
            FunctionCallError::RespondToModel(format!("Invalid glob pattern: {err}"))
        })?;
        Some(glob.compile_matcher())
    } else {
        None
    };

    // Run search in a blocking task with timeout
    let search_path = search_path.to_path_buf();
    let search_future = tokio::task::spawn_blocking(move || {
        search_files_native(
            &matcher,
            glob_matcher.as_ref(),
            &search_path,
            limit,
            include_hidden,
        )
    });

    let results = tokio::time::timeout(SEARCH_TIMEOUT, search_future)
        .await
        .map_err(|_| {
            FunctionCallError::RespondToModel("Search timed out after 30 seconds".to_string())
        })?
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!("Search task failed: {err}"))
        })??;

    Ok(results)
}

fn search_files_native(
    matcher: &RegexMatcher,
    glob_matcher: Option<&GlobMatcher>,
    search_path: &Path,
    limit: usize,
    include_hidden: bool,
) -> Result<Vec<String>, FunctionCallError> {
    let mut results_with_time: Vec<(String, std::time::SystemTime)> = Vec::new();

    // Build walker with .gitignore support.
    // By default we mimic ripgrep's behavior: skip hidden files/directories,
    // unless include_hidden is explicitly enabled.
    let walker = {
        let mut builder = WalkBuilder::new(search_path);
        if include_hidden {
            builder.hidden(false);
        }
        builder
            .git_ignore(true) // Respect .gitignore
            .git_global(true) // Respect global gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .build()
    };

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue, // Skip errors (like permission denied)
        };

        // Only process files
        if !entry.file_type().map_or(false, |ft| ft.is_file()) {
            continue;
        }

        let path = entry.path();

        // Apply glob filter if provided
        if let Some(glob) = glob_matcher {
            if !glob.is_match(path) {
                continue;
            }
        }

        // Search file content
        let mut searcher = Searcher::new();
        let mut found = false;

        let search_result = searcher.search_path(
            matcher,
            path,
            UTF8(|_lnum, _line| {
                found = true;
                Ok(false) // Stop after first match (--files-with-matches behavior)
            }),
        );

        // Skip files that can't be read or don't match
        if search_result.is_err() || !found {
            continue;
        }

        // Get file modification time for sorting
        let path_str = path.display().to_string();
        if let Ok(metadata) = std::fs::metadata(path) {
            if let Ok(modified) = metadata.modified() {
                results_with_time.push((path_str, modified));
            } else {
                results_with_time.push((path_str, std::time::SystemTime::UNIX_EPOCH));
            }
        }

        // Stop if we've reached the limit
        if results_with_time.len() >= limit {
            break;
        }
    }

    // Sort by modification time (newest first, like --sortr=modified)
    results_with_time.sort_by(|a, b| b.1.cmp(&a.1));

    // Extract just the paths
    let results: Vec<String> = results_with_time.into_iter().map(|(path, _)| path).collect();

    Ok(results)
}



#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn run_search_returns_results() -> anyhow::Result<()> {
        let temp = tempdir()?;
        let dir = temp.path();
        std::fs::write(dir.join("match_one.txt"), "alpha beta gamma")?;
        std::fs::write(dir.join("match_two.txt"), "alpha delta")?;
        std::fs::write(dir.join("other.txt"), "omega")?;

        let results = run_rg_search("alpha", None, false, dir, 10, dir).await?;
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|path| path.ends_with("match_one.txt")));
        assert!(results.iter().any(|path| path.ends_with("match_two.txt")));
        Ok(())
    }

    #[tokio::test]
    async fn run_search_with_glob_filter() -> anyhow::Result<()> {
        let temp = tempdir()?;
        let dir = temp.path();
        std::fs::write(dir.join("match_one.rs"), "alpha beta gamma")?;
        std::fs::write(dir.join("match_two.txt"), "alpha delta")?;

        let results = run_rg_search("alpha", Some("*.rs"), false, dir, 10, dir).await?;
        assert_eq!(results.len(), 1);
        assert!(results.iter().all(|path| path.ends_with("match_one.rs")));
        Ok(())
    }

    #[tokio::test]
    async fn run_search_respects_limit() -> anyhow::Result<()> {
        let temp = tempdir()?;
        let dir = temp.path();
        std::fs::write(dir.join("one.txt"), "alpha one")?;
        std::fs::write(dir.join("two.txt"), "alpha two")?;
        std::fs::write(dir.join("three.txt"), "alpha three")?;

        let results = run_rg_search("alpha", None, false, dir, 2, dir).await?;
        assert_eq!(results.len(), 2);
        Ok(())
    }

    #[tokio::test]
    async fn run_search_handles_no_matches() -> anyhow::Result<()> {
        let temp = tempdir()?;
        let dir = temp.path();
        std::fs::write(dir.join("one.txt"), "omega")?;

        let results = run_rg_search("alpha", None, false, dir, 5, dir).await?;
        assert!(results.is_empty());
        Ok(())
    }
}
