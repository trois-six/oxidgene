//! The seam between App Settings and the desktop binary's `mcp` subcommand.
//!
//! The App Settings API section (`docs/specifications/ui-app-settings.md`
//! §8) shows how to point an MCP client — Claude Desktop, Claude Code, or any
//! other stdio client — at this application's assistant server. Building
//! that server, and knowing the absolute path of the running executable, are
//! both desktop-only: the web build has no local process to launch and no
//! filesystem path to report. So, like the Geneanet login window
//! ([`crate::geneanet`]) and the custom theme folder ([`crate::theme`]), the
//! capability is declared here as a value the desktop binary puts into the
//! Dioxus context, and the page reads it back with
//! [`use_assistant_launcher`]. A web build provides none, and the section
//! renders a note that the assistant is available in the desktop application
//! instead of a command that could not run.
//!
//! The server itself, its tools, and the consent model are specified in
//! `docs/specifications/mcp.md`.

use dioxus::prelude::*;

/// Desktop-only capability: how an MCP client launches this application's
/// assistant server.
#[derive(Clone, Debug, PartialEq)]
pub struct AssistantLauncher {
    /// Absolute path of the running desktop executable, as resolved by the
    /// desktop binary at startup.
    pub executable: String,
}

impl AssistantLauncher {
    /// The command line an MCP client runs to launch the assistant server:
    /// `<executable> mcp` (Assistant Access §4.1).
    ///
    /// The executable is quoted with double quotes when its path contains
    /// whitespace, so the line can be pasted into a shell as-is.
    pub fn command_line(&self) -> String {
        let executable = &self.executable;
        if executable.chars().any(char::is_whitespace) {
            format!("\"{executable}\" mcp")
        } else {
            format!("{executable} mcp")
        }
    }

    /// The pretty-printed `mcpServers` JSON client configuration
    /// (Assistant Access §8).
    pub fn client_config_json(&self) -> String {
        let config = serde_json::json!({
            "mcpServers": {
                "oxidgene": {
                    "command": self.executable,
                    "args": ["mcp"],
                }
            }
        });
        serde_json::to_string_pretty(&config).expect("a string-only JSON value always serializes")
    }
}

/// The assistant launcher, if this build has one.
///
/// `None` on the web target, and on any desktop build that did not install
/// one. The App Settings API section treats both the same way — it shows the
/// desktop-only note rather than a command that could not run.
pub fn use_assistant_launcher() -> Option<AssistantLauncher> {
    try_use_context::<AssistantLauncher>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launcher(executable: &str) -> AssistantLauncher {
        AssistantLauncher {
            executable: executable.to_string(),
        }
    }

    #[test]
    fn command_line_is_the_executable_followed_by_mcp() {
        assert_eq!(
            launcher("/opt/oxidgene/oxidgene-desktop").command_line(),
            "/opt/oxidgene/oxidgene-desktop mcp"
        );
    }

    #[test]
    fn command_line_quotes_a_path_containing_whitespace() {
        assert_eq!(
            launcher("/Applications/OxidGene Desktop/oxidgene-desktop").command_line(),
            "\"/Applications/OxidGene Desktop/oxidgene-desktop\" mcp"
        );
    }

    #[test]
    fn client_config_json_matches_the_specified_shape() {
        let json = launcher("/opt/oxidgene/oxidgene-desktop").client_config_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed,
            serde_json::json!({
                "mcpServers": {
                    "oxidgene": {
                        "command": "/opt/oxidgene/oxidgene-desktop",
                        "args": ["mcp"]
                    }
                }
            })
        );
        // Pretty-printed, so it reads as a snippet to paste into a config file.
        assert!(json.contains('\n'));
    }

    #[test]
    fn client_config_json_keeps_a_path_with_whitespace_unquoted_inside_the_string() {
        let json = launcher("/Applications/OxidGene Desktop/oxidgene-desktop").client_config_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed["mcpServers"]["oxidgene"]["command"],
            "/Applications/OxidGene Desktop/oxidgene-desktop"
        );
    }
}
