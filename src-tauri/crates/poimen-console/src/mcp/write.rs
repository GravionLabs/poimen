//! `write_terminal` (#1184): types text into an existing terminal that is not an agent. Every call
//! needs the user's confirmation in the app, except in a terminal the caller started itself with
//! `start_shell` (`Terminal.started_by`); there is no setting that turns the question off.

use poimen_client::Client;
use poimen_client::protocol::{AgentWriteAction, TerminalStatus};
use serde_json::{Value, json};

use super::stop::{find, has_running_agent};
use super::{Env, Fail, announce, authorize, runs, server_error, text_arg};

/// The tool definition of `tools/list`.
pub(super) fn tool() -> Value {
    json!({
        "name": "write_terminal",
        "description": "Types text into an existing terminal that runs no agent, e.g. the `shell` pane of a space. DANGER: the text runs in the shell of the user, with their rights. Prefer start_shell for new work and use this only for a pane that is already there. Unless the terminal was started by you with start_shell, the user must confirm every call in the poimen app (the dialog shows the text; waits up to 60 s); there is no setting that turns this off. Control characters other than newline and tab are refused, so it cannot send signals or escape sequences: use stop_terminal to interrupt. A terminal with a running agent is refused: use prompt_agent. Watch the effect with read_terminal.",
        "inputSchema": {"type": "object", "properties": {
            "terminal": {"type": "string", "description": "Terminal id."},
            "text": {"type": "string", "description": "The text to type. Newline and tab only; no other control characters."},
            "enter": {"type": "boolean", "description": "Send Enter after the text (default false)."}
        }, "required": ["terminal", "text"], "additionalProperties": false},
    })
}

/// The first character of `text` that may not be typed: a control character other than newline
/// and tab (C0, DEL and C1, so no escape sequence and no signal).
fn forbidden(text: &str) -> Option<char> {
    text.chars()
        .find(|c| c.is_control() && *c != '\n' && *c != '\t')
}

/// `write_terminal {terminal, text, enter?}`.
pub(super) fn write_terminal(client: &Client, env: &Env, args: &Value) -> Result<Value, Fail> {
    runs::only_keys(args, &["terminal", "text", "enter"])?;
    let id = text_arg(args, "terminal")
        .ok_or_else(|| Fail::Tool("missing argument `terminal`".into()))?;
    let text = args
        .get("text")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| Fail::Tool("missing argument `text`".into()))?;
    let enter = match args.get("enter") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(Fail::Tool("`enter` must be a boolean".into())),
    };
    if let Some(c) = forbidden(text) {
        return Err(Fail::Tool(format!(
            "refused: `text` holds the control character U+{:04X}; only newline and tab are allowed \
             (use stop_terminal to interrupt)",
            c as u32
        )));
    }
    let Some(caller) = env.terminal_id.filter(|i| !i.is_empty()) else {
        return Err(Fail::Tool(
            "refused: this agent does not run in a poimen terminal (POIMEN_TERMINAL_ID is not set)"
                .into(),
        ));
    };
    let target = find(client, id)?
        .ok_or_else(|| Fail::Tool(format!("terminal {id} is not known to the server")))?;
    if has_running_agent(&target) {
        return Err(Fail::Tool(format!(
            "refused: terminal {id} runs an agent; use prompt_agent"
        )));
    }
    if target.status == TerminalStatus::Exited {
        return Err(Fail::Tool(format!("terminal {id} has exited")));
    }
    let own = target.started_by.as_deref() == Some(caller);
    let confirmed = if own {
        false
    } else {
        authorize(
            client,
            env,
            &target.space_id,
            AgentWriteAction::WriteTerminal,
            Some(id),
            text,
            true,
        )?
    };
    let mut data = text.to_owned();
    if enter {
        data.push('\r');
    }
    client.input(id, &data).map_err(server_error)?;
    announce(
        client,
        env,
        &target.space_id,
        AgentWriteAction::WriteTerminal,
        Some(id),
        text,
        confirmed,
    );
    Ok(
        json!({"terminal": id, "typed": text.chars().count(), "enter": enter, "confirmed": confirmed}),
    )
}

#[cfg(test)]
mod tests {
    use super::forbidden;

    #[test]
    fn only_newline_and_tab_pass() {
        assert_eq!(forbidden("ls -l\n\tx ü"), None);
        assert_eq!(forbidden("a\u{3}"), Some('\u{3}'));
        assert_eq!(forbidden("\u{1b}[A"), Some('\u{1b}'));
        assert_eq!(forbidden("a\r"), Some('\r'));
        assert_eq!(forbidden("\u{7f}"), Some('\u{7f}'));
        assert_eq!(forbidden("\u{9b}"), Some('\u{9b}'));
    }
}
