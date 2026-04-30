//! Hook-facing tool names and matcher compatibility aliases.
//!
//! Hook stdin exposes one canonical `tool_name`, but matcher selection may also
//! need to recognize names from adjacent tool ecosystems. Keeping those two
//! concepts together prevents handlers from accidentally serializing a
//! compatibility alias, such as `Write`, as the stable hook payload name.

use codex_tools::ToolName;

/// Identifies a tool in hook payloads and hook matcher selection.
///
/// `name` is the canonical value serialized into hook stdin. Matcher aliases are
/// internal-only compatibility names that may select the same hook handlers but
/// must not change the payload seen by hook processes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HookToolName {
    name: String,
    matcher_aliases: Vec<String>,
}

impl HookToolName {
    /// Builds a hook tool name with no matcher aliases.
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            matcher_aliases: Vec::new(),
        }
    }

    /// Builds the canonical hook-facing identity for function-style tools that
    /// do not need Claude-compatibility aliases.
    ///
    /// MCP tool names already use a stable fully qualified `mcp__...__tool`
    /// form, so we preserve them verbatim. Other namespaced tools use the
    /// `dynamic__namespace__tool` form so hooks can wildcard an entire dynamic
    /// namespace without colliding with plain tool names.
    pub(crate) fn for_function_tool(tool_name: &ToolName) -> Self {
        match tool_name.namespace.as_deref() {
            Some(namespace) if namespace.starts_with("mcp__") => Self::new(tool_name.display()),
            Some(namespace) => Self::new(format!("dynamic__{namespace}__{}", tool_name.name)),
            None => Self::new(tool_name.name.clone()),
        }
    }

    /// Returns the hook identity for file edits performed through `apply_patch`.
    ///
    /// The serialized name remains `apply_patch` so logs and policies can key
    /// off the actual Codex tool. `Write` and `Edit` are accepted as matcher
    /// aliases for compatibility with hook configurations that describe edits
    /// using Claude Code-style names.
    pub(crate) fn apply_patch() -> Self {
        Self {
            name: "apply_patch".to_string(),
            matcher_aliases: vec!["Write".to_string(), "Edit".to_string()],
        }
    }

    /// Returns the hook identity historically used for shell-like tools.
    pub(crate) fn bash() -> Self {
        Self::new("Bash")
    }

    /// Returns the canonical hook name serialized into hook stdin.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Returns additional matcher inputs that should select the same handlers.
    pub(crate) fn matcher_aliases(&self) -> &[String] {
        &self.matcher_aliases
    }
}

#[cfg(test)]
mod tests {
    use super::HookToolName;
    use codex_tools::ToolName;
    use pretty_assertions::assert_eq;

    #[test]
    fn for_function_tool_keeps_plain_tool_names() {
        assert_eq!(
            HookToolName::for_function_tool(&ToolName::plain("tool_search")),
            HookToolName::new("tool_search"),
        );
    }

    #[test]
    fn for_function_tool_keeps_mcp_names_stable() {
        assert_eq!(
            HookToolName::for_function_tool(&ToolName::namespaced(
                "mcp__memory__",
                "create_entities",
            )),
            HookToolName::new("mcp__memory__create_entities"),
        );
    }

    #[test]
    fn for_function_tool_prefixes_dynamic_namespaces() {
        assert_eq!(
            HookToolName::for_function_tool(&ToolName::namespaced(
                "codex_app",
                "automation_update",
            )),
            HookToolName::new("dynamic__codex_app__automation_update"),
        );
    }
}
