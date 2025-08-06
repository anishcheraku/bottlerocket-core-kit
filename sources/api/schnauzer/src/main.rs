/*!
# Multicall Binary for schnauzer

This binary serves as both schnauzer (v1) and schnauzer-v2 based on how it's invoked.
The dispatch mechanism checks argv[0] to determine which tool to run.

schnauzer is called by sundog as a setting generator.
schnauzer-v2 is a more advanced settings generator for rendering handlebars templates.

(The name "schnauzer" comes from the fact that Schnauzers are search and rescue dogs
(similar to this search and replace task) and because they have mustaches.)
*/

const SCHNAUZER_NAME: &str = "schnauzer";
const SCHNAUZER_V2_NAME: &str = "schnauzer-v2";
const DEFAULT_TOOL_NAME: &str = SCHNAUZER_V2_NAME;

use snafu::ResultExt;
use std::env;

/// Enumeration of available tools in this multicall binary
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolName {
    SchnauzerV1,
    SchnauzerV2,
}

impl ToolName {
    /// Determine which tool to run based on the program name
    fn from_program_name(name: &str) -> Self {
        match name {
            SCHNAUZER_NAME => Self::SchnauzerV1,
            _ => Self::SchnauzerV2,
        }
    }
}

/// Extract program name from argv[0] for tool dispatch
fn extract_program_name() -> String {
    env::args()
        .next()
        .and_then(|path| {
            std::path::Path::new(&path)
                .file_name()
                .and_then(|name| name.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| DEFAULT_TOOL_NAME.to_string())
}

#[snafu::report]
#[tokio::main]
async fn main() -> Result<(), snafu::Whatever> {
    let program_name = extract_program_name();
    let tool = ToolName::from_program_name(&program_name);

    // Dispatch to the appropriate CLI module
    match tool {
        ToolName::SchnauzerV1 => schnauzer::v1::cli::run()
            .await
            .whatever_context("schnauzer v1 execution failed"),
        ToolName::SchnauzerV2 => schnauzer::v2::cli::run()
            .await
            .whatever_context("schnauzer v2 execution failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_name_from_program_name() {
        // Given Various program names
        // When Converting program names to ToolName enum
        // Then Should correctly identify schnauzer v1 vs v2

        // Test schnauzer v1 detection
        assert_eq!(
            ToolName::from_program_name("schnauzer"),
            ToolName::SchnauzerV1
        );

        // Test schnauzer v2 detection
        assert_eq!(
            ToolName::from_program_name("schnauzer-v2"),
            ToolName::SchnauzerV2
        );

        // Test default behavior (unknown names default to v2)
        assert_eq!(
            ToolName::from_program_name("unknown"),
            ToolName::SchnauzerV2
        );
        assert_eq!(ToolName::from_program_name(""), ToolName::SchnauzerV2);
    }
}
