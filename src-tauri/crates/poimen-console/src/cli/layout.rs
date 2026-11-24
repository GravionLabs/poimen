//! Parsing of `poimen space ...` and `poimen tab ...` (#1172).

use poimen_protocol::{SplitDirection, valid_ratio};

use super::{Command, Request, exactly, split_dashdash};

/// How `space new` answers the trust question of a project file with commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustFlag {
    /// `--trust`: run the commands (stored for this content hash).
    Trust,
    /// `--without-commands`: build the panes without them.
    Without,
}

pub(super) fn parse_space(args: &[String]) -> Result<Command, String> {
    let Some((sub, rest)) = args.split_first() else {
        return Err("missing space command (new, rename, close)".into());
    };
    let request = match sub.as_str() {
        "new" => {
            let (mut dir, mut name, mut trust) = (None, None, None);
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--name" => name = Some(it.next().ok_or("--name needs a value")?.clone()),
                    "--trust" | "--without-commands" => {
                        let flag = if a == "--trust" {
                            TrustFlag::Trust
                        } else {
                            TrustFlag::Without
                        };
                        if trust.replace(flag).is_some_and(|t| t != flag) {
                            return Err("--trust and --without-commands exclude each other".into());
                        }
                    }
                    flag if flag.starts_with('-') => {
                        return Err(format!("unknown option `{flag}`"));
                    }
                    word if dir.is_none() => dir = Some(word.to_string()),
                    _ => return Err("only one directory can be given".into()),
                }
            }
            Request::SpaceNew {
                dir: dir.ok_or("missing directory")?,
                name,
                trust,
            }
        }
        "rename" => {
            let [space, name] = exactly(rest.to_vec(), "space and name")?;
            Request::SpaceRename { space, name }
        }
        "close" => {
            let (yes, pos) = flag_and_words(rest, "--yes")?;
            let [space] = exactly(pos, "space")?;
            Request::SpaceClose { space, yes }
        }
        other => return Err(format!("unknown space command `{other}`")),
    };
    Ok(Command::Request(request))
}

pub(super) fn parse_tab(args: &[String]) -> Result<Command, String> {
    let Some((sub, rest)) = args.split_first() else {
        return Err("missing tab command (new, rename, close, split, focus)".into());
    };
    let request = match sub.as_str() {
        "new" => {
            let (own, command) = command_tail(rest)?;
            let (space, name) = positional_and_name(own, "space")?;
            Request::TabNew {
                space,
                name,
                command,
            }
        }
        "rename" => {
            let [tab, name] = exactly(rest.to_vec(), "tab and name")?;
            Request::TabRename { tab, name }
        }
        "close" => {
            let (yes, pos) = flag_and_words(rest, "--yes")?;
            let [tab] = exactly(pos, "tab")?;
            Request::TabClose { tab, yes }
        }
        "split" => parse_split(rest)?,
        "focus" => {
            let mut words = rest.to_vec();
            if let Some(f) = words.iter().find(|w| w.starts_with('-')) {
                return Err(format!("unknown option `{f}`"));
            }
            if words.is_empty() {
                return Err("missing tab".into());
            }
            if words.len() > 2 {
                return Err("too many arguments".into());
            }
            let terminal = (words.len() == 2).then(|| words.remove(1));
            Request::TabFocus {
                tab: words.remove(0),
                terminal,
            }
        }
        other => return Err(format!("unknown tab command `{other}`")),
    };
    Ok(Command::Request(request))
}

fn parse_split(rest: &[String]) -> Result<Request, String> {
    let (own, command) = command_tail(rest)?;
    let (mut words, mut ratio) = (Vec::new(), None);
    let mut it = own.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--ratio" => {
                let v = it.next().ok_or("--ratio needs a value")?;
                let r: f64 = v
                    .parse()
                    .map_err(|_| format!("--ratio is a number, got `{v}`"))?;
                if !valid_ratio(r) {
                    return Err(format!("--ratio is above 0 and below 1, got `{v}`"));
                }
                ratio = Some(r);
            }
            flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}`")),
            word => words.push(word.to_string()),
        }
    }
    let [terminal, direction] = exactly(words, "terminal and direction (right or down)")?;
    let direction = match direction.as_str() {
        "right" => SplitDirection::Right,
        "down" => SplitDirection::Down,
        other => return Err(format!("direction is right or down, got `{other}`")),
    };
    Ok(Request::TabSplit {
        terminal,
        direction,
        ratio,
        command,
    })
}

/// The words before `--` and the command after it (never empty when given).
fn command_tail(args: &[String]) -> Result<(&[String], Option<Vec<String>>), String> {
    let (own, tail) = split_dashdash(args);
    if args.iter().any(|a| a == "--") && tail.is_empty() {
        return Err("missing command after `--`".into());
    }
    Ok((own, (!tail.is_empty()).then(|| tail.to_vec())))
}

fn positional_and_name(own: &[String], what: &str) -> Result<(String, Option<String>), String> {
    let (mut pos, mut name) = (None, None);
    let mut it = own.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--name" => name = Some(it.next().ok_or("--name needs a value")?.clone()),
            flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}`")),
            word if pos.is_none() => pos = Some(word.to_string()),
            _ => return Err(format!("only one {what} can be given")),
        }
    }
    Ok((pos.ok_or(format!("missing {what}"))?, name))
}

fn flag_and_words(args: &[String], flag: &str) -> Result<(bool, Vec<String>), String> {
    super::switch(args, flag)
}

#[cfg(test)]
mod tests {
    use super::super::parse;
    use super::*;

    fn p(args: &[&str]) -> Result<Request, String> {
        match parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())? {
            Command::Request(r) => Ok(r),
            other => Err(format!("{other:?}")),
        }
    }

    fn s(x: &str) -> String {
        x.to_string()
    }

    #[test]
    fn space_commands() {
        assert_eq!(
            p(&["space", "new", "/w", "--name", "n", "--trust"]),
            Ok(Request::SpaceNew {
                dir: s("/w"),
                name: Some(s("n")),
                trust: Some(TrustFlag::Trust)
            })
        );
        assert_eq!(
            p(&["space", "new", "--without-commands", "/w"]),
            Ok(Request::SpaceNew {
                dir: s("/w"),
                name: None,
                trust: Some(TrustFlag::Without)
            })
        );
        assert!(p(&["space", "new", "/w", "--trust", "--without-commands"]).is_err());
        assert!(p(&["space", "new"]).is_err());
        assert!(p(&["space", "new", "/a", "/b"]).is_err());
        assert_eq!(
            p(&["space", "rename", "a", "b"]),
            Ok(Request::SpaceRename {
                space: s("a"),
                name: s("b")
            })
        );
        assert!(p(&["space", "rename", "a"]).is_err());
        assert_eq!(
            p(&["space", "close", "a"]),
            Ok(Request::SpaceClose {
                space: s("a"),
                yes: false
            })
        );
        assert_eq!(
            p(&["space", "close", "--yes", "a"]),
            Ok(Request::SpaceClose {
                space: s("a"),
                yes: true
            })
        );
        assert!(p(&["space"]).is_err());
        assert!(p(&["space", "x"]).is_err());
    }

    #[test]
    fn tab_new_rename_close_focus() {
        assert_eq!(
            p(&["tab", "new", "w", "--name", "n", "--", "sh", "-c", "x"]),
            Ok(Request::TabNew {
                space: s("w"),
                name: Some(s("n")),
                command: Some(vec![s("sh"), s("-c"), s("x")])
            })
        );
        assert_eq!(
            p(&["tab", "new", "w"]),
            Ok(Request::TabNew {
                space: s("w"),
                name: None,
                command: None
            })
        );
        assert!(p(&["tab", "new", "w", "--"]).is_err());
        assert!(p(&["tab", "new"]).is_err());
        assert_eq!(
            p(&["tab", "rename", "t", "x"]),
            Ok(Request::TabRename {
                tab: s("t"),
                name: s("x")
            })
        );
        assert_eq!(
            p(&["tab", "close", "t", "--yes"]),
            Ok(Request::TabClose {
                tab: s("t"),
                yes: true
            })
        );
        assert_eq!(
            p(&["tab", "focus", "t"]),
            Ok(Request::TabFocus {
                tab: s("t"),
                terminal: None
            })
        );
        assert_eq!(
            p(&["tab", "focus", "t", "t1"]),
            Ok(Request::TabFocus {
                tab: s("t"),
                terminal: Some(s("t1"))
            })
        );
        assert!(p(&["tab", "focus", "t", "a", "b"]).is_err());
        assert!(p(&["tab"]).is_err());
    }

    #[test]
    fn tab_split_options() {
        assert_eq!(
            p(&["tab", "split", "t1", "down", "--ratio", "0.3", "--", "top"]),
            Ok(Request::TabSplit {
                terminal: s("t1"),
                direction: SplitDirection::Down,
                ratio: Some(0.3),
                command: Some(vec![s("top")])
            })
        );
        assert_eq!(
            p(&["tab", "split", "t1", "right"]),
            Ok(Request::TabSplit {
                terminal: s("t1"),
                direction: SplitDirection::Right,
                ratio: None,
                command: None
            })
        );
        assert!(p(&["tab", "split", "t1", "left"]).is_err());
        assert!(p(&["tab", "split", "t1"]).is_err());
        assert!(p(&["tab", "split", "t1", "right", "--ratio", "1"]).is_err());
        assert!(p(&["tab", "split", "t1", "right", "--ratio", "x"]).is_err());
    }
}
