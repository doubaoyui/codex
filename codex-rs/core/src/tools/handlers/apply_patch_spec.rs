use codex_tools::FreeformTool;
use codex_tools::FreeformToolFormat;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;

const APPLY_PATCH_LARK_GRAMMAR: &str = include_str!("apply_patch.lark");

const APPLY_PATCH_JSON_TOOL_DESCRIPTION: &str = r#"Use the `apply_patch` tool to edit files.
Your patch language is a stripped-down, file-oriented diff format designed to be easy to parse and safe to apply. A patch must use this envelope:

*** Begin Patch
[ one or more file sections ]
*** End Patch

Within that envelope, each file operation starts with one of these headers:

*** Add File: <path> - create a new file. Every following line is a + line.
*** Delete File: <path> - remove an existing file. Nothing follows.
*** Update File: <path> - patch an existing file in place, optionally followed by *** Move to: <new path>.

Update hunks start with @@ and contain lines prefixed with a space, -, or +. File references must be relative, never absolute.
"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyPatchToolArgs {
    pub input: String,
}

/// Returns a custom tool that can be used to edit files. Well-suited for GPT-5 models
/// https://platform.openai.com/docs/guides/function-calling#custom-tools
pub fn create_apply_patch_freeform_tool(include_environment_id: bool) -> ToolSpec {
    let definition = if include_environment_id {
        APPLY_PATCH_LARK_GRAMMAR.replace(
            "start: begin_patch hunk+ end_patch",
            "start: begin_patch environment_id? hunk+ end_patch\nenvironment_id: \"*** Environment ID: \" filename LF",
        )
    } else {
        APPLY_PATCH_LARK_GRAMMAR.to_string()
    };
    ToolSpec::Freeform(FreeformTool {
        name: "apply_patch".to_string(),
        description: "Use the `apply_patch` tool to edit files. This is a FREEFORM tool, so do not wrap the patch in JSON.".to_string(),
        format: FreeformToolFormat {
            r#type: "grammar".to_string(),
            syntax: "lark".to_string(),
            definition,
        },
    })
}

/// Returns a JSON function tool for models that cannot emit Responses custom/freeform tools.
pub fn create_apply_patch_json_tool(include_environment_id: bool) -> ToolSpec {
    let input_description = if include_environment_id {
        "The entire apply_patch patch body. To target a non-primary environment, put `*** Environment ID: <id>` immediately after `*** Begin Patch`."
    } else {
        "The entire apply_patch patch body."
    };
    let description = if include_environment_id {
        format!(
            "{APPLY_PATCH_JSON_TOOL_DESCRIPTION}\nWhen this turn exposes multiple environments, choose a non-primary environment by placing `*** Environment ID: <id>` immediately after `*** Begin Patch`."
        )
    } else {
        APPLY_PATCH_JSON_TOOL_DESCRIPTION.to_string()
    };
    let properties = BTreeMap::from([(
        "input".to_string(),
        JsonSchema::string(Some(input_description.to_string())),
    )]);

    ToolSpec::Function(ResponsesApiTool {
        name: "apply_patch".to_string(),
        description,
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec!["input".to_string()]),
            Some(false.into()),
        ),
        output_schema: None,
    })
}

#[cfg(test)]
#[path = "apply_patch_spec_tests.rs"]
mod tests;
