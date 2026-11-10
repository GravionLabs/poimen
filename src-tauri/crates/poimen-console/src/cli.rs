//! Arguments of `poimen attach`.

use poimen_protocol::AttachMode;

pub const USAGE: &str = "usage: poimen <command>\n\
\n\
  ls [--json]                         spaces, terminals and agent status\n\
  status <terminal> [--json]          one terminal: state and agent status\n\
  new <space> [--name <n>] [-- cmd..] a new terminal in a space (id or name); prints its id\n\
  send <terminal> [--enter] <text..>  types the text as it is (--enter adds Enter)\n\
  prompt <terminal> <text..>          prompts the agent; refused while it is working or blocked\n\
  -V, --version                       prints the version\n\
  attach [--control [--takeover]] <terminal>\n\
                                      shows a terminal; observes by default, --control sends your\n\
                                      input and sets the size, --takeover replaces a controller.\n\
                                      Detach with Ctrl+B d (Ctrl+B Ctrl+B sends Ctrl+B)\n\
\n\
Inside a poimen terminal the server comes from POIMEN_SOCKET, else the default socket.";

#[derive(Debug, PartialEq, Eq)]
pub struct AttachArgs {
    pub terminal_id: String,
    pub mode: AttachMode,
    pub take_over: bool,
}

/// The commands that talk to the server in single requests (everything but `attach`).
#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    Ls {
        json: bool,
    },
    Status {
        terminal: String,
        json: bool,
    },
    New {
        space: String,
        name: Option<String>,
        command: Option<Vec<String>>,
    },
    Send {
        terminal: String,
        text: String,
        enter: bool,
    },
    Prompt {
        terminal: String,
        text: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Attach(AttachArgs),
    Request(Request),
    Help,
    Version,
}

/// Parses the arguments after the program name. `Err` is a usage message (exit code 2).
pub fn parse(args: &[String]) -> Result<Command, String> {
    let Some((name, rest)) = args.split_first() else {
        return Ok(Command::Help);
    };
    if wants_help(name, rest) {
        return Ok(Command::Help);
    }
    match name.as_str() {
        "-h" | "--help" | "help" => Ok(Command::Help),
        "-V" | "--version" => Ok(Command::Version),
        "attach" => parse_attach(rest),
        "ls" => {
            let (json, pos) = switch(rest, "--json")?;
            no_more(&pos, 0)?;
            Ok(Command::Request(Request::Ls { json }))
        }
        "status" => {
            let (json, pos) = switch(rest, "--json")?;
            let [terminal] = exactly(pos, "terminal id")?;
            Ok(Command::Request(Request::Status { terminal, json }))
        }
        "new" => parse_new(rest),
        "send" => {
            let (opts, tail) = split_dashdash(rest);
            let (enter, mut pos) = switch(opts, "--enter")?;
            pos.extend(tail.iter().cloned());
            let (terminal, text) = terminal_and_text(pos)?;
            Ok(Command::Request(Request::Send {
                terminal,
                text,
                enter,
            }))
        }
        "prompt" => {
            let (opts, tail) = split_dashdash(rest);
            let mut pos = opts.to_vec();
            pos.extend(tail.iter().cloned());
            let (terminal, text) = terminal_and_text(pos)?;
            Ok(Command::Request(Request::Prompt { terminal, text }))
        }
        other => Err(format!("unknown command `{other}`")),
    }
}

/// `-h`/`--help` asks for help only where it is an option: never after `--`, and for `send` and
/// `prompt` (free text) only before the first positional argument.
fn wants_help(name: &str, rest: &[String]) -> bool {
    let free_text = matches!(name, "send" | "prompt");
    for a in rest {
        if a == "--" {
            return false;
        }
        if a == "-h" || a == "--help" {
            return true;
        }
        if free_text && !a.starts_with('-') {
            return false;
        }
    }
    false
}

/// Splits at the first `--`: the options part and the words after it (taken as they are).
fn split_dashdash(args: &[String]) -> (&[String], &[String]) {
    match args.iter().position(|a| a == "--") {
        Some(i) => (&args[..i], &args[i + 1..]),
        None => (args, &[]),
    }
}

/// Splits the switch `flag` from the words; any other `--option` is an error.
fn switch(args: &[String], flag: &str) -> Result<(bool, Vec<String>), String> {
    let mut on = false;
    let mut words = Vec::new();
    for a in args {
        if a == flag {
            on = true;
        } else if a.starts_with("--") {
            return Err(format!("unknown option `{a}`"));
        } else {
            words.push(a.clone());
        }
    }
    Ok((on, words))
}

fn no_more(words: &[String], max: usize) -> Result<(), String> {
    if words.len() > max {
        return Err(format!("unexpected argument `{}`", words[max]));
    }
    Ok(())
}

fn exactly<const N: usize>(words: Vec<String>, what: &str) -> Result<[String; N], String> {
    let n = words.len();
    words.try_into().map_err(|_| {
        if n < N {
            format!("missing {what}")
        } else {
            "too many arguments".to_string()
        }
    })
}

fn terminal_and_text(words: Vec<String>) -> Result<(String, String), String> {
    let mut it = words.into_iter();
    let terminal = it.next().ok_or("missing terminal id")?;
    let text = it.collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err("missing text".into());
    }
    Ok((terminal, text))
}

fn parse_new(args: &[String]) -> Result<Command, String> {
    let (own, command) = match args.iter().position(|a| a == "--") {
        Some(i) => (&args[..i], Some(args[i + 1..].to_vec())),
        None => (args, None),
    };
    if command.as_ref().is_some_and(Vec::is_empty) {
        return Err("missing command after `--`".into());
    }
    let (mut space, mut name) = (None, None);
    let mut it = own.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--name" => name = Some(it.next().ok_or("--name needs a value")?.clone()),
            flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}`")),
            word if space.is_none() => space = Some(word.to_string()),
            _ => return Err("only one space can be given".into()),
        }
    }
    Ok(Command::Request(Request::New {
        space: space.ok_or("missing space")?,
        name,
        command,
    }))
}

fn parse_attach(args: &[String]) -> Result<Command, String> {
    let (mut control, mut take_over, mut terminal) = (false, false, None);
    for a in args {
        match a.as_str() {
            "--control" => control = true,
            "--takeover" => take_over = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}`")),
            id if terminal.is_none() => terminal = Some(id.to_string()),
            _ => return Err("only one terminal can be attached".into()),
        }
    }
    let terminal_id = terminal.ok_or("missing terminal id")?;
    if take_over && !control {
        return Err("--takeover needs --control".into());
    }
    Ok(Command::Attach(AttachArgs {
        terminal_id,
        mode: if control {
            AttachMode::Control
        } else {
            AttachMode::Observe
        },
        take_over,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn help_after_dashdash_is_the_command() {
        let Ok(Command::Request(Request::New { command, .. })) =
            p(&["new", "work", "--", "grep", "--help"])
        else {
            panic!()
        };
        assert_eq!(
            command,
            Some(vec!["grep".to_string(), "--help".to_string()])
        );
        assert_eq!(p(&["new", "work", "-h"]), Ok(Command::Help));
    }

    #[test]
    fn help_inside_text_is_text() {
        assert_eq!(
            p(&["send", "t1", "-h"]),
            Ok(Command::Request(Request::Send {
                terminal: "t1".into(),
                text: "-h".into(),
                enter: false
            }))
        );
        assert_eq!(
            p(&["prompt", "t1", "explain", "--help"]),
            Ok(Command::Request(Request::Prompt {
                terminal: "t1".into(),
                text: "explain --help".into()
            }))
        );
        assert_eq!(p(&["send", "--help"]), Ok(Command::Help));
        assert_eq!(p(&["prompt", "-h", "t1"]), Ok(Command::Help));
    }

    #[test]
    fn dashdash_ends_options_for_send_and_prompt() {
        assert_eq!(
            p(&["send", "--enter", "t1", "--", "--version"]),
            Ok(Command::Request(Request::Send {
                terminal: "t1".into(),
                text: "--version".into(),
                enter: true
            }))
        );
        assert_eq!(
            p(&["prompt", "t1", "--", "--help", "me"]),
            Ok(Command::Request(Request::Prompt {
                terminal: "t1".into(),
                text: "--help me".into()
            }))
        );
    }

    #[test]
    fn observes_by_default() {
        assert_eq!(
            p(&["attach", "t1"]),
            Ok(Command::Attach(AttachArgs {
                terminal_id: "t1".into(),
                mode: AttachMode::Observe,
                take_over: false
            }))
        );
    }

    #[test]
    fn control_and_takeover() {
        let Ok(Command::Attach(a)) = p(&["attach", "--control", "--takeover", "t1"]) else {
            panic!()
        };
        assert_eq!(a.mode, AttachMode::Control);
        assert!(a.take_over);
    }

    #[test]
    fn takeover_alone_is_refused() {
        assert!(p(&["attach", "--takeover", "t1"]).is_err());
    }

    #[test]
    fn version_flags() {
        assert_eq!(p(&["--version"]), Ok(Command::Version));
        assert_eq!(p(&["-V"]), Ok(Command::Version));
    }

    #[test]
    fn errors() {
        assert!(p(&["attach"]).is_err());
        assert!(p(&["attach", "a", "b"]).is_err());
        assert!(p(&["attach", "--nope", "a"]).is_err());
        assert!(p(&["bogus"]).is_err());
        assert_eq!(p(&[]), Ok(Command::Help));
    }

    fn req(args: &[&str]) -> Request {
        match p(args) {
            Ok(Command::Request(r)) => r,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn ls_and_status_take_json() {
        assert_eq!(req(&["ls"]), Request::Ls { json: false });
        assert_eq!(req(&["ls", "--json"]), Request::Ls { json: true });
        assert_eq!(
            req(&["status", "--json", "t1"]),
            Request::Status {
                terminal: "t1".into(),
                json: true
            }
        );
        assert!(p(&["ls", "x"]).is_err());
        assert!(p(&["status"]).is_err());
        assert!(p(&["status", "a", "b"]).is_err());
        assert!(p(&["status", "--nope", "a"]).is_err());
    }

    #[test]
    fn new_takes_the_command_after_the_dashes() {
        assert_eq!(
            req(&["new", "work", "--name", "build", "--", "make", "-j", "4"]),
            Request::New {
                space: "work".into(),
                name: Some("build".into()),
                command: Some(vec!["make".into(), "-j".into(), "4".into()]),
            }
        );
        assert_eq!(
            req(&["new", "s1"]),
            Request::New {
                space: "s1".into(),
                name: None,
                command: None
            }
        );
        assert!(p(&["new"]).is_err());
        assert!(p(&["new", "s1", "--"]).is_err());
        assert!(p(&["new", "a", "b"]).is_err());
        assert!(p(&["new", "a", "--name"]).is_err());
    }

    #[test]
    fn send_and_prompt_join_the_words() {
        assert_eq!(
            req(&["send", "t1", "--enter", "ls", "-la"]),
            Request::Send {
                terminal: "t1".into(),
                text: "ls -la".into(),
                enter: true
            }
        );
        assert_eq!(
            req(&["prompt", "t1", "fix", "the bug"]),
            Request::Prompt {
                terminal: "t1".into(),
                text: "fix the bug".into()
            }
        );
        assert!(p(&["send", "t1"]).is_err());
        assert!(p(&["prompt"]).is_err());
        assert_eq!(p(&["send", "--help"]), Ok(Command::Help));
    }
}
