//! The `usage` setting (#784).

use serde::Serialize;
use serde_json::Value;

/// The `usage` setting: which provider limits the status bar asks for (#784).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Usage {
    /// `usage.claude`: ask the Claude usage endpoint (default true); false reads no token and
    /// sends no request.
    pub claude: bool,
}

impl Default for Usage {
    fn default() -> Self {
        Self { claude: true }
    }
}

pub(super) fn resolve_usage(value: &Value, warnings: &mut Vec<String>) -> Usage {
    let mut usage = Usage::default();
    let Some(object) = value.as_object() else {
        warnings.push("usage must be an object".into());
        return usage;
    };
    match object.get("claude") {
        None => {}
        Some(Value::Bool(b)) => usage.claude = *b,
        Some(other) => warnings.push(format!("usage.claude {other} must be true or false")),
    }
    usage
}
