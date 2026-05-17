use super::*;
use pretty_assertions::assert_eq;

#[test]
fn create_apply_patch_freeform_tool_matches_expected_spec() {
    assert_eq!(
        create_apply_patch_freeform_tool(/*include_environment_id*/ false),
        ToolSpec::Freeform(FreeformTool {
            name: "apply_patch".to_string(),
            description:
                "Use the `apply_patch` tool to edit files. This is a FREEFORM tool, so do not wrap the patch in JSON."
                    .to_string(),
            format: FreeformToolFormat {
                r#type: "grammar".to_string(),
                syntax: "lark".to_string(),
                definition: APPLY_PATCH_LARK_GRAMMAR.to_string(),
            },
        })
    );
}

#[test]
fn create_apply_patch_freeform_tool_includes_environment_id_when_requested() {
    let ToolSpec::Freeform(tool) =
        create_apply_patch_freeform_tool(/*include_environment_id*/ true)
    else {
        panic!("expected freeform tool");
    };

    assert!(tool.format.definition.contains("environment_id?"));
    assert!(
        tool.format
            .definition
            .contains("\"*** Environment ID: \" filename LF")
    );
}

#[test]
fn create_apply_patch_json_tool_matches_expected_shape() {
    let ToolSpec::Function(tool) =
        create_apply_patch_json_tool(/*include_environment_id*/ false)
    else {
        panic!("expected function tool");
    };

    assert_eq!(tool.name, "apply_patch");
    assert!(!tool.strict);
    let Some(properties) = tool.parameters.properties else {
        panic!("expected object properties");
    };
    assert_eq!(
        properties.get("input"),
        Some(&JsonSchema::string(Some(
            "The entire apply_patch patch body.".to_string()
        )))
    );
    assert_eq!(tool.parameters.required, Some(vec!["input".to_string()]));
    assert_eq!(tool.parameters.additional_properties, Some(false.into()));
}

#[test]
fn create_apply_patch_json_tool_mentions_environment_id_when_requested() {
    let ToolSpec::Function(tool) =
        create_apply_patch_json_tool(/*include_environment_id*/ true)
    else {
        panic!("expected function tool");
    };

    assert!(tool.description.contains("*** Environment ID: <id>"));
    let Some(properties) = tool.parameters.properties else {
        panic!("expected object properties");
    };
    assert!(
        properties
            .get("input")
            .and_then(|schema| schema.description.as_deref())
            .is_some_and(|description| description.contains("*** Environment ID: <id>"))
    );
}
