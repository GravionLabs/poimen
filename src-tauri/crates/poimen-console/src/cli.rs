//! Arguments of `poimen`: none for the TUI, else a command.

use poimen_protocol::{AttachMode, Placement, SplitDirection};

mod layout;
pub use layout::TrustFlag;

/// A raw string: a `\` line continuation would drop the indentation of the next line.
pub const USAGE: &str = r#"usage: poimen [<command>]

  (none)                              the TUI: spaces and terminals, the selected one attached.
                                      Ctrl+B then: d detaches, : palette, N notifications,
                                      c new tab, % " split, h j k l focus, Up/Down select,
                                      b sidebar (console.prefix, console.keys)
  ls [--json]                         spaces, terminals and agent status
  status <terminal> [--json]          one terminal: state and agent status
  new <space> [--name <n>] [-- cmd..] a new terminal in a space (id or name); prints its id
  space new <dir> [--name <n>] [--trust|--without-commands]
                                      a new space (layout of the project); prints its id. A project
                                      file with untrusted commands: exit 1 listing them and the
                                      hash, unless --trust runs them or --without-commands skips
  space rename <space> <name>         renames a space (id or name)
  space close <space> [--yes]         closes a space and its terminals; --yes when an agent runs
  tab new <space> [--name <n>] [-- cmd..]
                                      a new tab (id or name of the space); prints the tab id
  tab rename <tab> <name>             renames a tab (id or name)
  tab close <tab> [--yes]             closes a tab and its terminals; --yes when an agent runs
  tab split <terminal> right|down [--ratio <r>] [-- cmd..]
                                      splits the terminal's pane; prints the new terminal id
  tab focus <tab> [<terminal>]        focuses a tab (and a terminal in it) for every client
  send <terminal> [--enter] <text..>  types the text as it is (--enter adds Enter)
  prompt <terminal> <text..>          prompts the agent; refused while it is working or blocked
  session run --harness <id> --prompt <text> [--model <m>] [--cwd <dir>] [--space <id>]
              [--run <id>] [--task <id>] [--env KEY=VALUE].. [--name <n>]
              [--placement editor|panel] [--close-on-end]
                                      runs one agent session in a new terminal and waits for its
                                      end (ADR 0016). Prints one JSON object per line on stdout:
                                      {"type":"started","terminal":..,"space":..}, {"type":"status",
                                      "status":"working|blocked|done|error|gone"} per change and
                                      {"type":"ended","status":"done|error|gone","terminal":..}
                                      last. --space defaults to the space of POIMEN_TERMINAL_ID.
                                      SIGINT/SIGTERM end the wait and leave the terminal running;
                                      --close-on-end ends the agent (its exitInput) and closes the
                                      terminal instead. Exit: 0 done, 2 usage, 3 start failed,
                                      4 error or gone, 5 connection lost, 6 permission refused,
                                      130 SIGINT, 143 SIGTERM
  machines [--json]                   the machines of config.json and whether their server answers
  mcp                                 a stdio MCP server for agents: read tools, start_agent,
                                      prompt_agent, wait_agent, report_done
  doctor [--json] [--bundle <file>]   checks console, server, PTY host, state, agent hooks, tools;
                                      --bundle writes a local diagnostics file (never uploaded)
  -V, --version                       prints the version
  attach [--control [--takeover]] <terminal>
                                      shows a terminal; observes by default, --control sends your
                                      input and sets the size, --takeover replaces a controller.
                                      Detach with Ctrl+B d (Ctrl+B Ctrl+B sends Ctrl+B)

--machine <name> before any command (or alone, for the TUI) uses the poimen-server of a machine
of config.json (`machines`) over ssh instead of the local one. mcp and doctor stay local.
Inside a poimen terminal the server comes from POIMEN_SOCKET, else the default socket.
Guide: https://gravionlabs.github.io/poimen/guide/console"#;

#[derive(Debug, PartialEq, Eq)]
pub struct AttachArgs {
    pub terminal_id: String,
    pub mode: AttachMode,
    pub take_over: bool,
}

/// The arguments of `poimen session run` (#1257).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SessionRunArgs {
    pub harness: String,
    pub prompt: String,
    pub model: Option<String>,
    pub cwd: Option<String>,
    pub space: Option<String>,
    pub run: Option<String>,
    pub task: Option<String>,
    pub env: Vec<(String, String)>,
    pub name: Option<String>,
    pub placement: Option<Placement>,
    pub close_on_end: bool,
}

/// The commands that talk to the server in single requests (everything but `attach`).
#[derive(Debug, PartialEq)]
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
    /// `space new <dir> [--name n] [--trust|--without-commands]` (#1172).
    SpaceNew {
        dir: String,
        name: Option<String>,
        trust: Option<TrustFlag>,
    },
    SpaceRename {
        space: String,
        name: String,
    },
    SpaceClose {
        space: String,
        yes: bool,
    },
    TabNew {
        space: String,
        name: Option<String>,
        command: Option<Vec<String>>,
    },
    TabRename {
        tab: String,
        name: String,
    },
    TabClose {
        tab: String,
        yes: bool,
    },
    TabSplit {
        terminal: String,
        direction: SplitDirection,
        ratio: Option<f64>,
        command: Option<Vec<String>>,
    },
    TabFocus {
        tab: String,
        terminal: Option<String>,
    },
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Attach(AttachArgs),
    Request(Request),
    /// A stdio MCP server (ADR 0012).
    Mcp,
    /// `poimen session run` (#1257, ADR 0016).
    SessionRun(SessionRunArgs),
    /// `poimen machines [--json]` (#1176).
    Machines {
        json: bool,
    },
    /// `poimen doctor` (#972).
    Doctor {
        json: bool,
        bundle: Option<String>,
    },
    /// No arguments: the TUI.
    Tui,
    Help,
    Version,
}

/// The arguments with the global `--machine <name>` (#1176) taken off the front.
#[derive(Debug, PartialEq)]
pub struct Invocation {
    pub machine: Option<String>,
    pub command: Command,
}

/// [`parse`] after a leading `--machine <name>` or `--machine=<name>`. `mcp` and `doctor` are
/// local only: with a machine they are a usage error.
pub fn parse_invocation(args: &[String]) -> Result<Invocation, String> {
    let (machine, rest) = match args.split_first() {
        Some((first, rest)) if first == "--machine" => match rest.split_first() {
            Some((name, rest)) if !name.is_empty() && !name.starts_with('-') => {
                (Some(name.clone()), rest)
            }
            _ => return Err("--machine needs a machine name".into()),
        },
        Some((first, rest)) if first.starts_with("--machine=") => {
            let name = &first["--machine=".len()..];
            if name.is_empty() {
                return Err("--machine needs a machine name".into());
            }
            (Some(name.to_owned()), rest)
        }
        _ => (None, args),
    };
    let command = parse(rest)?;
    if machine.is_some() {
        match command {
            Command::Mcp => return Err("`poimen mcp` always uses the local server".into()),
            Command::Doctor { .. } => {
                return Err("`poimen doctor` checks this machine only".into());
            }
            Command::Machines { .. } => {
                return Err("`poimen machines` lists the machines; it takes no --machine".into());
            }
            _ => {}
        }
    }
    Ok(Invocation { machine, command })
}

/// Parses the arguments after the program name. `Err` is a usage message (exit code 2).
pub fn parse(args: &[String]) -> Result<Command, String> {
    let Some((name, rest)) = args.split_first() else {
        return Ok(Command::Tui);
    };
    if wants_help(name, rest) {
        return Ok(Command::Help);
    }
    match name.as_str() {
        "-h" | "--help" | "help" => Ok(Command::Help),
        "-V" | "--version" => Ok(Command::Version),
        "attach" => parse_attach(rest),
        "session" => parse_session(rest),
        "space" => layout::parse_space(rest),
        "tab" => layout::parse_tab(rest),
        "mcp" => no_more(rest, 0).map(|()| Command::Mcp),
        "doctor" => parse_doctor(rest),
        "machines" => {
            let (json, pos) = switch(rest, "--json")?;
            no_more(&pos, 0)?;
            Ok(Command::Machines { json })
        }
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
    let mut value_next = false;
    for a in rest {
        // The value of a `session run` option (a prompt) may be `--help`.
        if std::mem::take(&mut value_next) {
            continue;
        }
        if name == "session" && SESSION_VALUE_OPTIONS.contains(&a.as_str()) {
            value_next = true;
            continue;
        }
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

const SESSION_VALUE_OPTIONS: [&str; 9] = [
    "--harness",
    "--model",
    "--prompt",
    "--cwd",
    "--space",
    "--run",
    "--task",
    "--env",
    "--name",
];

fn parse_session(args: &[String]) -> Result<Command, String> {
    match args.split_first() {
        Some((sub, rest)) if sub == "run" => parse_session_run(rest),
        Some((sub, _)) => Err(format!("unknown session command `{sub}`")),
        None => Err("missing session command (run)".into()),
    }
}

fn parse_session_run(args: &[String]) -> Result<Command, String> {
    let mut a = SessionRunArgs::default();
    let (mut harness, mut prompt) = (None, None);
    let mut it = args.iter();
    while let Some(opt) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{opt} needs a value"))
        };
        match opt.as_str() {
            "--harness" => harness = Some(value()?),
            "--prompt" => prompt = Some(value()?),
            "--model" => a.model = Some(value()?),
            "--cwd" => a.cwd = Some(value()?),
            "--space" => a.space = Some(value()?),
            "--run" => a.run = Some(value()?),
            "--task" => a.task = Some(value()?),
            "--name" => a.name = Some(value()?),
            "--env" => {
                let kv = value()?;
                match kv.split_once('=') {
                    Some((k, v)) if !k.is_empty() => a.env.push((k.to_string(), v.to_string())),
                    _ => return Err(format!("--env needs KEY=VALUE, got `{kv}`")),
                }
            }
            "--placement" => {
                a.placement = Some(match value()?.as_str() {
                    "editor" => Placement::Editor,
                    "panel" => Placement::Panel,
                    other => return Err(format!("--placement is editor or panel, got `{other}`")),
                });
            }
            "--close-on-end" => a.close_on_end = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}`")),
            word => return Err(format!("unexpected argument `{word}`")),
        }
    }
    a.harness = harness
        .filter(|h| !h.is_empty())
        .ok_or("missing --harness")?;
    a.prompt = prompt.ok_or("missing --prompt")?;
    Ok(Command::SessionRun(a))
}

fn parse_doctor(args: &[String]) -> Result<Command, String> {
    let (mut json, mut bundle) = (false, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--bundle" => bundle = Some(it.next().ok_or("--bundle needs a file")?.clone()),
            other if other.starts_with('-') => return Err(format!("unknown option `{other}`")),
            other => return Err(format!("unexpected argument `{other}`")),
        }
    }
    Ok(Command::Doctor { json, bundle })
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
    fn doctor_options() {
        let d = |json, bundle: Option<&str>| {
            Ok(Command::Doctor {
                json,
                bundle: bundle.map(String::from),
            })
        };
        assert_eq!(p(&["doctor"]), d(false, None));
        assert_eq!(p(&["doctor", "--json"]), d(true, None));
        assert_eq!(
            p(&["doctor", "--bundle", "out.json"]),
            d(false, Some("out.json"))
        );
        assert_eq!(
            p(&["doctor", "--json", "--bundle", "o"]),
            d(true, Some("o"))
        );
        assert!(p(&["doctor", "--bundle"]).is_err());
        assert!(p(&["doctor", "--x"]).is_err());
        assert!(p(&["doctor", "extra"]).is_err());
        assert_eq!(p(&["doctor", "--help"]), Ok(Command::Help));
    }

    fn session(args: &[&str]) -> Result<SessionRunArgs, String> {
        let mut all = vec!["session", "run"];
        all.extend(args);
        match p(&all)? {
            Command::SessionRun(a) => Ok(a),
            other => Err(format!("{other:?}")),
        }
    }

    #[test]
    fn session_run_options() {
        let a = session(&[
            "--harness",
            "claude",
            "--prompt",
            "fix it's \"$(x); y\nnow",
            "--model",
            "opus",
            "--cwd",
            "/w",
            "--space",
            "s1",
            "--run",
            "r1",
            "--task",
            "t1",
            "--env",
            "A=1",
            "--env",
            "B=x=y",
            "--name",
            "tab",
            "--placement",
            "panel",
            "--close-on-end",
        ])
        .unwrap();
        assert_eq!(a.harness, "claude");
        assert_eq!(a.prompt, "fix it's \"$(x); y\nnow");
        assert_eq!(a.model.as_deref(), Some("opus"));
        assert_eq!(a.cwd.as_deref(), Some("/w"));
        assert_eq!(a.space.as_deref(), Some("s1"));
        assert_eq!(a.run.as_deref(), Some("r1"));
        assert_eq!(a.task.as_deref(), Some("t1"));
        assert_eq!(
            a.env,
            vec![("A".into(), "1".into()), ("B".into(), "x=y".into())]
        );
        assert_eq!(a.name.as_deref(), Some("tab"));
        assert_eq!(a.placement, Some(Placement::Panel));
        assert!(a.close_on_end);
        let min = session(&["--harness", "claude", "--prompt", "go"]).unwrap();
        assert_eq!((min.model, min.space, min.placement), (None, None, None));
        assert!(!min.close_on_end);
    }

    #[test]
    fn session_run_rejects_bad_arguments() {
        for bad in [
            &["--prompt", "go"][..],
            &["--harness", "claude"],
            &["--harness", "claude", "--prompt"],
            &["--harness", "claude", "--prompt", "go", "--env", "NOEQ"],
            &["--harness", "claude", "--prompt", "go", "--env", "=v"],
            &["--harness", "claude", "--prompt", "go", "--placement", "x"],
            &["--harness", "claude", "--prompt", "go", "--bogus"],
            &["--harness", "claude", "--prompt", "go", "extra"],
        ] {
            assert!(session(bad).is_err(), "{bad:?}");
        }
        assert!(p(&["session"]).is_err());
        assert!(p(&["session", "stop"]).is_err());
    }

    #[test]
    fn session_run_prompt_may_be_a_help_flag() {
        assert!(session(&["--harness", "claude", "--prompt", "--help"]).is_ok());
        assert_eq!(p(&["session", "run", "--help"]), Ok(Command::Help));
        assert_eq!(p(&["session", "--help"]), Ok(Command::Help));
    }

    #[test]
    fn mcp_takes_no_arguments() {
        assert_eq!(p(&["mcp"]), Ok(Command::Mcp));
        assert!(p(&["mcp", "x"]).is_err());
        assert_eq!(p(&["mcp", "--help"]), Ok(Command::Help));
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
    fn usage_keeps_the_column_indentation() {
        let line = |start: &str| {
            USAGE
                .lines()
                .find(|l| l.trim_start().starts_with(start))
                .unwrap_or_else(|| panic!("no usage line for {start}"))
                .to_owned()
        };
        assert!(line("ls [--json]").starts_with("  ls"));
        assert!(line("prompt_agent,").starts_with(&" ".repeat(38)));
        assert!(!line("mcp ").contains("read-only"));
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
        assert_eq!(p(&[]), Ok(Command::Tui));
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
