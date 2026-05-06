use std::fs;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use codex_tools::ToolName;

pub struct ListDirHandler;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 2000;
const DEFAULT_OFFSET: usize = 1;
const DEFAULT_DEPTH: usize = 1;
const MAX_DEPTH: usize = 8;

#[derive(Deserialize)]
struct ListDirArgs {
    dir_path: String,
    #[serde(default = "default_offset")]
    offset: usize,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default = "default_depth")]
    depth: usize,
}

const fn default_offset() -> usize {
    DEFAULT_OFFSET
}

const fn default_limit() -> usize {
    DEFAULT_LIMIT
}

const fn default_depth() -> usize {
    DEFAULT_DEPTH
}

impl ToolHandler for ListDirHandler {
    type Output = FunctionToolOutput;

    fn tool_name(&self) -> ToolName {
        ToolName::plain("list_dir")
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        let ToolInvocation { payload, .. } = invocation;
        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "list_dir handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: ListDirArgs = parse_arguments(&arguments)?;
        if args.offset == 0 {
            return Err(FunctionCallError::RespondToModel(
                "offset must be greater than zero".to_string(),
            ));
        }
        if args.limit == 0 {
            return Err(FunctionCallError::RespondToModel(
                "limit must be greater than zero".to_string(),
            ));
        }
        if args.depth == 0 {
            return Err(FunctionCallError::RespondToModel(
                "depth must be greater than zero".to_string(),
            ));
        }

        let root = PathBuf::from(&args.dir_path);
        if !root.is_absolute() {
            return Err(FunctionCallError::RespondToModel(
                "dir_path must be an absolute path".to_string(),
            ));
        }
        if !root.is_dir() {
            return Err(FunctionCallError::RespondToModel(format!(
                "`{}` is not a directory",
                root.display()
            )));
        }

        let mut entries = Vec::new();
        collect_entries(&root, &root, args.depth.min(MAX_DEPTH), &mut entries)?;
        entries.sort_by(|a, b| a.path.cmp(&b.path));

        let start = args.offset.saturating_sub(1);
        let limit = args.limit.min(MAX_LIMIT);
        let mut lines = vec![format!("Absolute path: {}", root.display())];
        for (index, entry) in entries.into_iter().skip(start).take(limit).enumerate() {
            let number = args.offset + index;
            lines.push(format!("{number}. [{}] {}", entry.kind, entry.path));
        }

        Ok(FunctionToolOutput::from_text(lines.join("\n"), Some(true)))
    }
}

struct DirEntryLine {
    kind: &'static str,
    path: String,
}

fn collect_entries(
    root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<DirEntryLine>,
) -> Result<(), FunctionCallError> {
    if depth == 0 {
        return Ok(());
    }

    let read_dir = fs::read_dir(dir).map_err(|err| {
        FunctionCallError::RespondToModel(format!("failed to list `{}`: {err}", dir.display()))
    })?;

    for entry in read_dir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .display()
            .to_string();
        let kind = if file_type.is_dir() {
            "dir"
        } else if file_type.is_file() {
            "file"
        } else {
            "other"
        };
        out.push(DirEntryLine { kind, path: rel });

        if file_type.is_dir() {
            collect_entries(root, &path, depth - 1, out)?;
        }
    }

    Ok(())
}
