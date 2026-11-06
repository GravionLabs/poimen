//! `poimen-server report ...`: lets a tool inside a poimen terminal (the deliver loop, a
//! script) show its state in the app. Like the hooks it never breaks the caller: outside a
//! poimen terminal, or with no server answering, it does nothing and exits 0; only a usage
//! error exits 2.
//!
//! - `report tokens [--terminal <id>] [--ttl <s>] [--source <s>] name=value ...`: sets tokens
//!   (`name=` with an empty value removes the token); `--source` defaults to `cli`.
//! - `report clear [--terminal <id>] --source <s>`: removes every token of the source.
//! - `report agent --state working|blocked|idle --source <s> --agent <name> [--terminal <id>]`:
//!   the lifecycle of an agent that has no hooks (`terminal.report_agent`). `--source` names the
//!   reporter in a script's own bookkeeping; the protocol does not carry it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use poimen_client::Client;
use poimen_protocol::{
    AgentStatus, MAX_TOKENS_PER_REPORT, TerminalClearTokens, TerminalReportAgent,
    TerminalReportTokens, valid_token_name,
};
use serde_json::Value;

/// Source of `report tokens` without `--source`.
pub const DEFAULT_SOURCE: &str = "cli";

pub const USAGE: &str = "usage: poimen-server report tokens [--terminal <id>] [--ttl <s>] \
[--source <s>] name=value ...\n       poimen-server report clear [--terminal <id>] --source <s>\n       \
poimen-server report agent --state working|blocked|idle --source <s> --agent <name> \
[--terminal <id>]";

/// What a `report` command line asks for.
#[derive(Debug, PartialEq)]
pub enum Plan {
    /// Not in a poimen terminal: nothing to do.
    Skip,
    Call {
        method: &'static str,
        params: Value,
    },
}

/// Turns the arguments after `report` into a [`Plan`]; `env_terminal` is `$POIMEN_TERMINAL_ID`
/// and `nanos` the `seq` of an agent report.
pub fn plan(args: &[String], env_terminal: Option<String>, nanos: u64) -> Result<Plan, String> {
    let Some((kind, rest)) = args.split_first() else {
        return Err("missing subcommand".into());
    };
    let mut terminal = None;
    let mut ttl = None;
    let mut source = None;
    let mut state = None;
    let mut agent = None;
    let mut tokens: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut it = rest.iter();
    let allowed: &[&str] = match kind.as_str() {
        "tokens" => &["--terminal", "--ttl", "--source"],
        "clear" => &["--terminal", "--source"],
        "agent" => &["--terminal", "--source", "--state", "--agent"],
        other => return Err(format!("unknown subcommand {other}")),
    };
    while let Some(arg) = it.next() {
        if arg.starts_with("--") {
            if !allowed.contains(&arg.as_str()) {
                return Err(format!("unknown option {arg}"));
            }
            let value = it
                .next()
                .ok_or_else(|| format!("{arg} needs a value"))?
                .clone();
            match arg.as_str() {
                "--terminal" => terminal = Some(value),
                "--ttl" => {
                    ttl = Some(
                        value
                            .parse::<u64>()
                            .ok()
                            .filter(|s| *s > 0)
                            .ok_or("--ttl must be a whole number of seconds above 0")?,
                    )
                }
                "--source" => source = Some(value),
                "--state" => state = Some(value),
                _ => agent = Some(value),
            }
        } else if kind == "tokens" {
            let (name, value) = arg
                .split_once('=')
                .ok_or_else(|| format!("{arg}: expected name=value"))?;
            if !valid_token_name(name) {
                return Err(format!(
                    "invalid token name {name:?}: 1 to 32 ASCII letters, digits, _ or -"
                ));
            }
            tokens.insert(
                name.to_owned(),
                (!value.is_empty()).then(|| value.to_owned()),
            );
        } else {
            return Err(format!("unexpected argument {arg}"));
        }
    }
    if source.as_deref().is_some_and(|s| s.trim().is_empty()) {
        return Err("--source must not be empty".into());
    }
    let terminal_id = terminal.or(env_terminal).filter(|t| !t.is_empty());
    let params = |v: Result<Value, serde_json::Error>| v.map_err(|e| e.to_string());
    let call = |terminal_id: Option<String>, method, build: &dyn Fn(String) -> Result<Value, _>| {
        match terminal_id {
            Some(id) => Ok(Plan::Call {
                method,
                params: params(build(id))?,
            }),
            None => Ok(Plan::Skip),
        }
    };
    match kind.as_str() {
        "tokens" => {
            if tokens.is_empty() {
                return Err("no name=value given".into());
            }
            if tokens.len() > MAX_TOKENS_PER_REPORT {
                return Err(format!("at most {MAX_TOKENS_PER_REPORT} tokens per report"));
            }
            call(terminal_id, "terminal.report_tokens", &|terminal_id| {
                serde_json::to_value(TerminalReportTokens {
                    terminal_id,
                    tokens: tokens.clone(),
                    ttl_s: ttl,
                    source: source.clone().unwrap_or_else(|| DEFAULT_SOURCE.into()),
                })
            })
        }
        "clear" => {
            let source = source.ok_or("--source is required")?;
            call(terminal_id, "terminal.clear_tokens", &|terminal_id| {
                serde_json::to_value(TerminalClearTokens {
                    terminal_id,
                    source: source.clone(),
                })
            })
        }
        _ => {
            let status = match state.as_deref() {
                Some("working") => AgentStatus::Working,
                Some("blocked") => AgentStatus::Blocked,
                Some("idle") => AgentStatus::Idle,
                _ => return Err("--state must be working, blocked or idle".into()),
            };
            source.ok_or("--source is required")?;
            let agent = agent
                .filter(|a| !a.is_empty())
                .ok_or("--agent is required")?;
            call(terminal_id, "terminal.report_agent", &|terminal_id| {
                serde_json::to_value(TerminalReportAgent {
                    terminal_id,
                    seq: nanos,
                    status,
                    agent: Some(agent.clone()),
                    session_id: None,
                    message: None,
                })
            })
        }
    }
}

/// Runs `report`: the process exit code. A server that does not answer or refuses is a note on
/// stderr, never a failure.
pub fn run(args: &[String], env_terminal: Option<String>, socket: PathBuf) -> u8 {
    match plan(args, env_terminal, crate::claude::now_nanos()) {
        Err(message) => {
            eprintln!("poimen-server: {message}\n{USAGE}");
            2
        }
        Ok(Plan::Skip) => 0,
        Ok(Plan::Call { method, params }) => {
            if let Err(e) = Client::new(socket).request(method, params) {
                eprintln!("poimen-server: report failed: {e}");
            }
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn a(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_owned()).collect()
    }

    fn params(args: &[&str], env: Option<&str>) -> Result<Plan, String> {
        plan(&a(args), env.map(str::to_owned), 77)
    }

    #[test]
    fn tokens_take_the_terminal_from_the_environment() {
        let plan = params(
            &[
                "tokens",
                "--ttl",
                "30",
                "--source",
                "deliver",
                "deliver_step=build",
                "deliver=",
            ],
            Some("t3"),
        )
        .unwrap();
        assert_eq!(
            plan,
            Plan::Call {
                method: "terminal.report_tokens",
                params: json!({"terminal_id":"t3","ttl_s":30,"source":"deliver",
                    "tokens":{"deliver_step":"build","deliver":null}})
            }
        );
    }

    #[test]
    fn an_explicit_terminal_wins_and_the_source_defaults() {
        let Plan::Call { params, .. } =
            params(&["tokens", "--terminal", "t9", "a=b=c"], Some("t3")).unwrap()
        else {
            panic!("expected a call");
        };
        assert_eq!(
            params,
            json!({"terminal_id":"t9","source":"cli","tokens":{"a":"b=c"}})
        );
    }

    #[test]
    fn outside_a_poimen_terminal_nothing_happens() {
        assert_eq!(params(&["tokens", "a=b"], None).unwrap(), Plan::Skip);
        assert_eq!(params(&["tokens", "a=b"], Some("")).unwrap(), Plan::Skip);
        assert_eq!(
            params(&["clear", "--source", "x"], None).unwrap(),
            Plan::Skip
        );
    }

    #[test]
    fn clear_needs_a_source() {
        assert_eq!(
            params(&["clear", "--source", "deliver"], Some("t1")).unwrap(),
            Plan::Call {
                method: "terminal.clear_tokens",
                params: json!({"terminal_id":"t1","source":"deliver"})
            }
        );
        assert!(params(&["clear"], Some("t1")).is_err());
    }

    #[test]
    fn agent_reports_a_state() {
        assert_eq!(
            params(
                &[
                    "agent", "--state", "blocked", "--source", "s", "--agent", "aider"
                ],
                Some("t1")
            )
            .unwrap(),
            Plan::Call {
                method: "terminal.report_agent",
                params: json!({"terminal_id":"t1","seq":77,"status":"blocked","agent":"aider","session_id":null})
            }
        );
        assert!(
            params(
                &["agent", "--state", "done", "--source", "s", "--agent", "x"],
                Some("t1")
            )
            .is_err()
        );
        assert!(params(&["agent", "--state", "idle", "--agent", "x"], Some("t1")).is_err());
        assert!(params(&["agent", "--state", "idle", "--source", "s"], Some("t1")).is_err());
    }

    #[test]
    fn usage_errors_are_reported_even_outside_a_terminal() {
        for bad in [
            &["tokens"][..],
            &["tokens", "novalue"],
            &["tokens", "bad name=x"],
            &["tokens", "=x"],
            &["tokens", "--ttl", "0", "a=b"],
            &["tokens", "--ttl", "x", "a=b"],
            &["tokens", "--source", "", "a=b"],
            &["tokens", "--terminal"],
            &["tokens", "--state", "idle", "a=b"],
            &["clear", "a=b", "--source", "s"],
            &["nope"],
            &[],
        ] {
            assert!(params(bad, None).is_err(), "{bad:?}");
        }
        let many: Vec<String> = std::iter::once("tokens".to_owned())
            .chain((0..17).map(|i| format!("k{i}=v")))
            .collect();
        assert!(plan(&many, None, 0).is_err());
    }
}
