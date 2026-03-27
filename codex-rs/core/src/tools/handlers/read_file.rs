use std::collections::VecDeque;
use std::path::PathBuf;

use async_trait::async_trait;
use codex_utils_string::take_bytes_at_char_boundary;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

pub struct ReadFileHandler;

const MAX_LINE_LENGTH: usize = 500;
const TAB_WIDTH: usize = 4;
const COMMENT_PREFIXES: &[&str] = &["#", "//", "--"];

#[derive(Deserialize)]
struct ReadFileArgs {
    file_path: String,
    #[serde(default = "defaults::offset")]
    offset: usize,
    #[serde(default = "defaults::limit")]
    limit: usize,
    #[serde(default)]
    mode: ReadMode,
    #[serde(default)]
    indentation: Option<IndentationArgs>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum ReadMode {
    #[default]
    Slice,
    Indentation,
}

#[derive(Deserialize, Clone)]
struct IndentationArgs {
    #[serde(default)]
    anchor_line: Option<usize>,
    #[serde(default = "defaults::max_levels")]
    max_levels: usize,
    #[serde(default = "defaults::include_siblings")]
    include_siblings: bool,
    #[serde(default = "defaults::include_header")]
    include_header: bool,
    #[serde(default)]
    max_lines: Option<usize>,
}

#[derive(Clone, Debug)]
struct LineRecord {
    number: usize,
    raw: String,
    display: String,
    indent: usize,
}

impl LineRecord {
    fn trimmed(&self) -> &str {
        self.raw.trim_start()
    }

    fn is_blank(&self) -> bool {
        self.trimmed().is_empty()
    }

    fn is_comment(&self) -> bool {
        COMMENT_PREFIXES
            .iter()
            .any(|prefix| self.raw.trim().starts_with(prefix))
    }
}

#[async_trait]
impl ToolHandler for ReadFileHandler {
    type Output = FunctionToolOutput;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        let ToolInvocation { payload, .. } = invocation;
        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "read_file handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: ReadFileArgs = parse_arguments(&arguments)?;
        let ReadFileArgs {
            file_path,
            offset,
            limit,
            mode,
            indentation,
        } = args;

        if offset == 0 {
            return Err(FunctionCallError::RespondToModel(
                "offset must be a 1-indexed line number".to_string(),
            ));
        }
        if limit == 0 {
            return Err(FunctionCallError::RespondToModel(
                "limit must be greater than zero".to_string(),
            ));
        }

        let path = PathBuf::from(&file_path);
        if !path.is_absolute() {
            return Err(FunctionCallError::RespondToModel(
                "file_path must be an absolute path".to_string(),
            ));
        }

        let collected = match mode {
            ReadMode::Slice => slice::read(&path, offset, limit).await?,
            ReadMode::Indentation => {
                let indentation = indentation.unwrap_or_default();
                indentation::read_block(&path, offset, limit, indentation).await?
            }
        };

        Ok(FunctionToolOutput::from_text(collected.join("\n"), Some(true)))
    }
}

mod slice {
    use std::path::Path;
    use tokio::fs::File;
    use tokio::io::AsyncBufReadExt;
    use tokio::io::BufReader;

    use crate::function_tool::FunctionCallError;
    use crate::tools::handlers::read_file::format_line;

    pub async fn read(
        path: &Path,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<String>, FunctionCallError> {
        let file = File::open(path)
            .await
            .map_err(|err| FunctionCallError::RespondToModel(format!("failed to read file: {err}")))?;

        let mut reader = BufReader::new(file);
        let mut collected = Vec::new();
        let mut seen = 0usize;
        let mut buffer = Vec::new();

        loop {
            buffer.clear();
            let bytes_read = reader.read_until(b'\n', &mut buffer).await.map_err(|err| {
                FunctionCallError::RespondToModel(format!("failed to read file: {err}"))
            })?;

            if bytes_read == 0 {
                break;
            }

            if buffer.last() == Some(&b'\n') {
                buffer.pop();
                if buffer.last() == Some(&b'\r') {
                    buffer.pop();
                }
            }

            seen += 1;
            if seen < offset {
                continue;
            }
            if collected.len() == limit {
                break;
            }

            collected.push(format!("L{seen}: {}", format_line(&buffer)));
            if collected.len() == limit {
                break;
            }
        }

        if seen < offset {
            return Err(FunctionCallError::RespondToModel(
                "offset exceeds file length".to_string(),
            ));
        }

        Ok(collected)
    }
}

mod indentation {
    use std::collections::VecDeque;
    use std::path::Path;
    use tokio::fs::File;
    use tokio::io::AsyncBufReadExt;
    use tokio::io::BufReader;

    use crate::function_tool::FunctionCallError;
    use crate::tools::handlers::read_file::IndentationArgs;
    use crate::tools::handlers::read_file::LineRecord;
    use crate::tools::handlers::read_file::TAB_WIDTH;
    use crate::tools::handlers::read_file::format_line;
    use crate::tools::handlers::read_file::trim_empty_lines;

    pub async fn read_block(
        path: &Path,
        offset: usize,
        limit: usize,
        options: IndentationArgs,
    ) -> Result<Vec<String>, FunctionCallError> {
        let anchor_line = options.anchor_line.unwrap_or(offset);
        if anchor_line == 0 {
            return Err(FunctionCallError::RespondToModel(
                "anchor_line must be a 1-indexed line number".to_string(),
            ));
        }

        let guard_limit = options.max_lines.unwrap_or(limit);
        if guard_limit == 0 {
            return Err(FunctionCallError::RespondToModel(
                "max_lines must be greater than zero".to_string(),
            ));
        }

        let collected = collect_file_lines(path).await?;
        if collected.is_empty() || anchor_line > collected.len() {
            return Err(FunctionCallError::RespondToModel(
                "anchor_line exceeds file length".to_string(),
            ));
        }

        let anchor_index = anchor_line - 1;
        let effective_indents = compute_effective_indents(&collected);
        let anchor_indent = effective_indents[anchor_index];
        let min_indent = if options.max_levels == 0 {
            0
        } else {
            anchor_indent.saturating_sub(options.max_levels * TAB_WIDTH)
        };
        let final_limit = limit.min(guard_limit).min(collected.len());

        if final_limit == 1 {
            return Ok(vec![format!(
                "L{}: {}",
                collected[anchor_index].number, collected[anchor_index].display
            )]);
        }

        let mut i: isize = anchor_index as isize - 1;
        let mut j: usize = anchor_index + 1;
        let mut i_counter_min_indent = 0;
        let mut j_counter_min_indent = 0;
        let mut out = VecDeque::with_capacity(limit);
        out.push_back(&collected[anchor_index]);

        while out.len() < final_limit {
            let mut progressed = 0;

            if i >= 0 {
                let iu = i as usize;
                if effective_indents[iu] >= min_indent {
                    out.push_front(&collected[iu]);
                    progressed += 1;
                    i -= 1;

                    if effective_indents[iu] == min_indent && !options.include_siblings {
                        let allow_header_comment =
                            options.include_header && collected[iu].is_comment();
                        let can_take_line = allow_header_comment || i_counter_min_indent == 0;

                        if can_take_line {
                            i_counter_min_indent += 1;
                        } else {
                            out.pop_front();
                            progressed -= 1;
                            i = -1;
                        }
                    }

                    if out.len() >= final_limit {
                        break;
                    }
                } else {
                    i = -1;
                }
            }

            if j < collected.len() {
                let ju = j;
                if effective_indents[ju] >= min_indent {
                    out.push_back(&collected[ju]);
                    progressed += 1;
                    j += 1;

                    if effective_indents[ju] == min_indent && !options.include_siblings {
                        if j_counter_min_indent > 0 {
                            out.pop_back();
                            progressed -= 1;
                            j = collected.len();
                        }
                        j_counter_min_indent += 1;
                    }
                } else {
                    j = collected.len();
                }
            }

            if progressed == 0 {
                break;
            }
        }

        trim_empty_lines(&mut out);
        Ok(out
            .into_iter()
            .map(|record| format!("L{}: {}", record.number, record.display))
            .collect())
    }

    async fn collect_file_lines(path: &Path) -> Result<Vec<LineRecord>, FunctionCallError> {
        let file = File::open(path)
            .await
            .map_err(|err| FunctionCallError::RespondToModel(format!("failed to read file: {err}")))?;

        let mut reader = BufReader::new(file);
        let mut buffer = Vec::new();
        let mut lines = Vec::new();
        let mut number = 0usize;

        loop {
            buffer.clear();
            let bytes_read = reader.read_until(b'\n', &mut buffer).await.map_err(|err| {
                FunctionCallError::RespondToModel(format!("failed to read file: {err}"))
            })?;

            if bytes_read == 0 {
                break;
            }

            if buffer.last() == Some(&b'\n') {
                buffer.pop();
                if buffer.last() == Some(&b'\r') {
                    buffer.pop();
                }
            }

            number += 1;
            let raw = String::from_utf8_lossy(&buffer).into_owned();
            let indent = measure_indent(&raw);
            let display = format_line(&buffer);
            lines.push(LineRecord {
                number,
                raw,
                display,
                indent,
            });
        }

        Ok(lines)
    }

    fn compute_effective_indents(records: &[LineRecord]) -> Vec<usize> {
        let mut effective = Vec::with_capacity(records.len());
        let mut previous_indent = 0usize;
        for record in records {
            if record.is_blank() {
                effective.push(previous_indent);
            } else {
                previous_indent = record.indent;
                effective.push(previous_indent);
            }
        }
        effective
    }

    fn measure_indent(line: &str) -> usize {
        line.chars()
            .take_while(|c| matches!(c, ' ' | '\t'))
            .map(|c| if c == '\t' { TAB_WIDTH } else { 1 })
            .sum()
    }
}

fn format_line(bytes: &[u8]) -> String {
    let decoded = String::from_utf8_lossy(bytes);
    if decoded.len() > MAX_LINE_LENGTH {
        take_bytes_at_char_boundary(&decoded, MAX_LINE_LENGTH).to_string()
    } else {
        decoded.into_owned()
    }
}

fn trim_empty_lines(out: &mut VecDeque<&LineRecord>) {
    while matches!(out.front(), Some(line) if line.raw.trim().is_empty()) {
        out.pop_front();
    }
    while matches!(out.back(), Some(line) if line.raw.trim().is_empty()) {
        out.pop_back();
    }
}

mod defaults {
    use super::IndentationArgs;

    impl Default for IndentationArgs {
        fn default() -> Self {
            Self {
                anchor_line: None,
                max_levels: max_levels(),
                include_siblings: include_siblings(),
                include_header: include_header(),
                max_lines: None,
            }
        }
    }

    pub fn offset() -> usize {
        1
    }

    pub fn limit() -> usize {
        2000
    }

    pub fn max_levels() -> usize {
        0
    }

    pub fn include_siblings() -> bool {
        false
    }

    pub fn include_header() -> bool {
        true
    }
}

#[cfg(test)]
#[path = "read_file_tests.rs"]
mod tests;
