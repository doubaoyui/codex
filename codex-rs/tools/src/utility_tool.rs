use crate::JsonSchema;
use crate::ResponsesApiTool;
use crate::ToolSpec;
use std::collections::BTreeMap;

pub fn create_list_dir_tool() -> ToolSpec {
    let properties = BTreeMap::from([
        (
            "dir_path".to_string(),
            JsonSchema::string(Some("Absolute path to the directory to list.".to_string())),
        ),
        (
            "offset".to_string(),
            JsonSchema::number(Some(
                "The entry number to start listing from. Must be 1 or greater.".to_string(),
            )),
        ),
        (
            "limit".to_string(),
            JsonSchema::number(Some("The maximum number of entries to return.".to_string())),
        ),
        (
            "depth".to_string(),
            JsonSchema::number(Some(
                "The maximum directory depth to traverse. Must be 1 or greater.".to_string(),
            )),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: "list_dir".to_string(),
        description:
            "Lists entries in a local directory with 1-indexed entry numbers and simple type labels."
                .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(properties, Some(vec!["dir_path".to_string()]), Some(false.into())),
        output_schema: None,
    })
}

pub fn create_grep_files_tool() -> ToolSpec {
    let properties = BTreeMap::from([
        (
            "pattern".to_string(),
            JsonSchema::string(Some(
                "Regular expression pattern to search for.".to_string(),
            )),
        ),
        (
            "path".to_string(),
            JsonSchema::string(Some(
                "Optional absolute or workspace-relative path to search within.".to_string(),
            )),
        ),
        (
            "include".to_string(),
            JsonSchema::string(Some(
                "Optional glob filter such as `*.rs` or `src/**/*.ts`.".to_string(),
            )),
        ),
        (
            "include_hidden".to_string(),
            JsonSchema::boolean(Some(
                "Whether to include hidden files and directories.".to_string(),
            )),
        ),
        (
            "limit".to_string(),
            JsonSchema::number(Some(
                "Maximum number of matching files to return.".to_string(),
            )),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: "grep_files".to_string(),
        description: "Searches local files for a regex pattern and returns matching file paths."
            .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec!["pattern".to_string()]),
            Some(false.into()),
        ),
        output_schema: None,
    })
}

pub fn create_read_file_tool() -> ToolSpec {
    let indentation_properties = BTreeMap::from([
        (
            "anchor_line".to_string(),
            JsonSchema::number(Some(
                "Optional 1-indexed line to anchor indentation-mode expansion.".to_string(),
            )),
        ),
        (
            "max_levels".to_string(),
            JsonSchema::number(Some(
                "Maximum indentation levels to expand upward from the anchor.".to_string(),
            )),
        ),
        (
            "include_siblings".to_string(),
            JsonSchema::boolean(Some(
                "Whether to include sibling blocks at the same indentation level.".to_string(),
            )),
        ),
        (
            "include_header".to_string(),
            JsonSchema::boolean(Some(
                "Whether to include the containing header line for the block.".to_string(),
            )),
        ),
        (
            "max_lines".to_string(),
            JsonSchema::number(Some(
                "Optional guardrail for indentation-mode total lines.".to_string(),
            )),
        ),
    ]);
    let properties = BTreeMap::from([
        (
            "file_path".to_string(),
            JsonSchema::string(Some("Absolute path to the file to read.".to_string())),
        ),
        (
            "offset".to_string(),
            JsonSchema::number(Some(
                "1-indexed starting line number.".to_string(),
            )),
        ),
        (
            "limit".to_string(),
            JsonSchema::number(Some(
                "Maximum number of lines to return.".to_string(),
            )),
        ),
        (
            "mode".to_string(),
            JsonSchema::string(Some(
                "Read mode: `slice` or `indentation`.".to_string(),
            )),
        ),
        (
            "indentation".to_string(),
            JsonSchema::object(indentation_properties, None, Some(false.into())),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: "read_file".to_string(),
        description:
            "Reads lines from a local file by slice or indentation-aware block extraction."
                .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec!["file_path".to_string()]),
            Some(false.into()),
        ),
        output_schema: None,
    })
}

pub fn create_test_sync_tool() -> ToolSpec {
    let barrier_properties = BTreeMap::from([
        (
            "id".to_string(),
            JsonSchema::string(Some(
                "Identifier shared by concurrent calls that should rendezvous".to_string(),
            )),
        ),
        (
            "participants".to_string(),
            JsonSchema::number(Some(
                "Number of tool calls that must arrive before the barrier opens".to_string(),
            )),
        ),
        (
            "timeout_ms".to_string(),
            JsonSchema::number(Some(
                "Maximum time in milliseconds to wait at the barrier".to_string(),
            )),
        ),
    ]);

    let properties = BTreeMap::from([
        (
            "sleep_before_ms".to_string(),
            JsonSchema::number(Some(
                "Optional delay in milliseconds before any other action".to_string(),
            )),
        ),
        (
            "sleep_after_ms".to_string(),
            JsonSchema::number(Some(
                "Optional delay in milliseconds after completing the barrier".to_string(),
            )),
        ),
        (
            "barrier".to_string(),
            JsonSchema::object(
                barrier_properties,
                Some(vec!["id".to_string(), "participants".to_string()]),
                Some(false.into()),
            ),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: "test_sync_tool".to_string(),
        description: "Internal synchronization helper used by Codex integration tests.".to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(properties, /*required*/ None, Some(false.into())),
        output_schema: None,
    })
}

#[cfg(test)]
#[path = "utility_tool_tests.rs"]
mod tests;
