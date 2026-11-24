//! `poimen integration` (#1173).

/// The agent hooks and MCP entry of one kind.
#[derive(Debug, PartialEq, Eq)]
pub struct Integration {
    /// `install`, `uninstall` or `status`.
    pub action: String,
    /// One of `poimen_protocol::HOOK_KINDS`.
    pub kind: String,
    /// `--settings <path>`: the agent's file instead of its default.
    pub settings: Option<String>,
}

pub fn parse(args: &[String]) -> Result<super::Command, String> {
    let usage = || {
        format!(
            "usage: poimen integration install|uninstall|status {} [--settings <path>]",
            poimen_protocol::HOOK_KINDS.join("|")
        )
    };
    let [action, kind, rest @ ..] = args else {
        return Err(usage());
    };
    if !["install", "uninstall", "status"].contains(&action.as_str())
        || !poimen_protocol::HOOK_KINDS.contains(&kind.as_str())
    {
        return Err(usage());
    }
    let settings = match rest {
        [] => None,
        [flag, path] if flag == "--settings" => Some(path.clone()),
        _ => return Err(usage()),
    };
    Ok(super::Command::Integration(Integration {
        action: action.clone(),
        kind: kind.clone(),
        settings,
    }))
}

#[cfg(test)]
mod tests {
    use super::super::Command;
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        super::super::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn integration_install_uninstall_status() {
        let i = |action: &str, kind: &str, settings: Option<&str>| {
            Ok(Command::Integration(Integration {
                action: action.into(),
                kind: kind.into(),
                settings: settings.map(String::from),
            }))
        };
        assert_eq!(
            p(&["integration", "install", "claude"]),
            i("install", "claude", None)
        );
        assert_eq!(
            p(&["integration", "uninstall", "copilot", "--settings", "/x"]),
            i("uninstall", "copilot", Some("/x"))
        );
        assert_eq!(
            p(&["integration", "status", "gemini"]),
            i("status", "gemini", None)
        );
        for bad in [
            &["integration"][..],
            &["integration", "install"],
            &["integration", "install", "codex"],
            &["integration", "enable", "claude"],
            &["integration", "status", "claude", "--settings"],
            &["integration", "status", "claude", "x"],
        ] {
            assert!(
                p(bad).unwrap_err().starts_with("usage: poimen integration"),
                "{bad:?}"
            );
        }
    }
}
