use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use grep_regex::RegexMatcher;
use grep_searcher::Searcher;
use grep_searcher::sinks::UTF8;
use ignore::WalkBuilder;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::parse_arguments;
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
    type Output = FunctionToolOutput;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        let ToolInvocation { payload, turn, .. } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "grep_files handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: GrepFilesArgs = parse_arguments(&arguments)?;
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
        verify_path_exists(&search_path).await?;

        let include = args.include.as_deref().map(str::trim).and_then(|value| {
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        });

        let results = run_rg_search(
            pattern,
            include.as_deref(),
            &search_path,
            limit,
            &turn.cwd,
            args.include_hidden,
        )
        .await?;

        if results.is_empty() {
            Ok(FunctionToolOutput::from_text(
                "No matches found.".to_string(),
                Some(false),
            ))
        } else {
            Ok(FunctionToolOutput::from_text(results.join("\n"), Some(true)))
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
    search_path: &Path,
    limit: usize,
    _cwd: &Path,
    include_hidden: bool,
) -> Result<Vec<String>, FunctionCallError> {
    let matcher = RegexMatcher::new(pattern)
        .map_err(|err| FunctionCallError::RespondToModel(format!("Invalid regex pattern: {err}")))?;

    let glob_matcher: Option<GlobMatcher> = if let Some(glob_pattern) = include {
        let glob = Glob::new(glob_pattern)
            .map_err(|err| FunctionCallError::RespondToModel(format!("Invalid glob pattern: {err}")))?;
        Some(glob.compile_matcher())
    } else {
        None
    };

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

    tokio::time::timeout(SEARCH_TIMEOUT, search_future)
        .await
        .map_err(|_| FunctionCallError::RespondToModel("Search timed out after 30 seconds".to_string()))?
        .map_err(|err| FunctionCallError::RespondToModel(format!("Search task failed: {err}")))?
}

fn search_files_native(
    matcher: &RegexMatcher,
    glob_matcher: Option<&GlobMatcher>,
    search_path: &Path,
    limit: usize,
    include_hidden: bool,
) -> Result<Vec<String>, FunctionCallError> {
    let mut results_with_time: Vec<(String, std::time::SystemTime)> = Vec::new();
    let walker = {
        let mut builder = WalkBuilder::new(search_path);
        if include_hidden {
            builder.hidden(false);
        }
        builder.git_ignore(true).git_global(true).git_exclude(true).build()
    };

    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry.file_type().is_some_and(|file_type| file_type.is_file()) {
            continue;
        }

        let path = entry.path();
        if let Some(glob) = glob_matcher && !glob.is_match(path) {
            continue;
        }

        let mut searcher = Searcher::new();
        let mut found = false;
        let search_result = searcher.search_path(
            matcher,
            path,
            UTF8(|_lnum, _line| {
                found = true;
                Ok(false)
            }),
        );

        if search_result.is_err() || !found {
            continue;
        }

        let modified = std::fs::metadata(path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        results_with_time.push((path.display().to_string(), modified));

        if results_with_time.len() >= limit {
            break;
        }
    }

    results_with_time.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(results_with_time
        .into_iter()
        .map(|(path, _)| path)
        .collect())
}

#[cfg(test)]
#[path = "grep_files_tests.rs"]
mod tests;
