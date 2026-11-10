//! Arguments of `poimen attach`.

use poimen_protocol::AttachMode;

pub const USAGE: &str = "usage: poimen-console attach [--control [--takeover]] <terminal>\n\
\n\
Attaches to a terminal of poimen-server in this terminal. Observes by default (the size is the\n\
controller's); --control sends your input and sets the size; --takeover replaces an existing\n\
controller. Detach with Ctrl+B d (Ctrl+B Ctrl+B sends Ctrl+B); the terminal keeps running.";

#[derive(Debug, PartialEq, Eq)]
pub struct AttachArgs {
    pub terminal_id: String,
    pub mode: AttachMode,
    pub take_over: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Attach(AttachArgs),
    Help,
}

/// Parses the arguments after the program name. `Err` is a usage message (exit code 2).
pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut it = args.iter();
    match it.next().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => return Ok(Command::Help),
        Some("attach") => {}
        Some(other) => return Err(format!("unknown command `{other}`")),
    }
    let (mut control, mut take_over, mut terminal) = (false, false, None);
    for a in it {
        match a.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
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
    fn errors() {
        assert!(p(&["attach"]).is_err());
        assert!(p(&["attach", "a", "b"]).is_err());
        assert!(p(&["attach", "--nope", "a"]).is_err());
        assert!(p(&["bogus"]).is_err());
        assert_eq!(p(&[]), Ok(Command::Help));
    }
}
