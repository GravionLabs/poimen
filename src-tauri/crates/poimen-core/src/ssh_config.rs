//! The concrete host names of `~/.ssh/config` (#863), for the add form of a machine. Only the
//! names of `Host` lines are read: no `HostName`, `IdentityFile` or any other option is kept, and a
//! missing or unreadable file is an empty list. `Include` is followed (relative to `~/.ssh`, `~`
//! and `*`/`?` in the file name allowed, depth and file count bounded); patterns (`*`, `?`, `!`)
//! are left out; the lines of a `Match` block are not `Host` lines and are never listed.

use std::path::{Path, PathBuf};

const MAX_DEPTH: usize = 8;
const MAX_FILES: usize = 64;
const MAX_BYTES: u64 = 1 << 20;

/// The host names of `<home>/.ssh/config`, in file order, without duplicates.
pub fn hosts(home: &Path) -> Vec<String> {
    let dir = home.join(".ssh");
    let mut out = Vec::new();
    let mut files = 0;
    read_file(&dir.join("config"), &dir, home, 0, &mut files, &mut out);
    out
}

fn read_file(
    path: &Path,
    dir: &Path,
    home: &Path,
    depth: usize,
    files: &mut usize,
    out: &mut Vec<String>,
) {
    if depth > MAX_DEPTH || *files >= MAX_FILES {
        return;
    }
    *files += 1;
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let Some((keyword, rest)) = split_line(line) else {
            continue;
        };
        match keyword.to_ascii_lowercase().as_str() {
            "host" => {
                for word in words(rest) {
                    if is_pattern(&word) || word.is_empty() || out.contains(&word) {
                        continue;
                    }
                    out.push(word);
                }
            }
            "include" => {
                for word in words(rest) {
                    for file in expand(&word, dir, home) {
                        read_file(&file, dir, home, depth + 1, files, out);
                    }
                }
            }
            _ => {}
        }
    }
}

/// `keyword value` or `keyword=value`; comments and blank lines are `None`.
fn split_line(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let end = line.find(|c: char| c.is_whitespace() || c == '=')?;
    let rest = line[end..].trim_start_matches(|c: char| c.is_whitespace() || c == '=');
    Some((&line[..end], rest))
}

/// The words of a value; double quotes group, an unquoted `#` starts a comment.
fn words(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in value.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '#' if !quoted => break,
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

fn is_pattern(word: &str) -> bool {
    word.starts_with('!') || word.contains(['*', '?'])
}

/// The files an `Include` operand names, sorted per pattern; a missing one is skipped.
fn expand(operand: &str, dir: &Path, home: &Path) -> Vec<PathBuf> {
    let path = match operand.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => {
            let p = Path::new(operand);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                dir.join(p)
            }
        }
    };
    let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
    else {
        return Vec::new();
    };
    if !name.contains(['*', '?']) {
        return vec![path];
    }
    // Wildcards are supported in the file name only, as in `Include conf.d/*`.
    if parent.to_string_lossy().contains(['*', '?']) {
        return Vec::new();
    }
    let Ok(read) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_str().is_some_and(|n| glob(name, n)))
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

/// `*` and `?` matching of one file name.
fn glob(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('*', rest)) => (0..=t.len()).any(|i| go(rest, &t[i..])),
            Some(('?', rest)) => !t.is_empty() && go(rest, &t[1..]),
            Some((c, rest)) => t.first() == Some(c) && go(rest, &t[1..]),
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    go(&p, &t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home_with(files: &[(&str, &str)]) -> tempfile_dir::Dir {
        let d = tempfile_dir::Dir::new();
        for (name, body) in files {
            let p = d.path().join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        d
    }

    /// A tiny temp dir without a new dependency.
    mod tempfile_dir {
        use std::path::{Path, PathBuf};
        pub struct Dir(PathBuf);
        impl Dir {
            pub fn new() -> Self {
                use std::sync::atomic::{AtomicU32, Ordering};
                static N: AtomicU32 = AtomicU32::new(0);
                let p = std::env::temp_dir().join(format!(
                    "poimen-ssh-config-{}-{}",
                    std::process::id(),
                    N.fetch_add(1, Ordering::SeqCst)
                ));
                std::fs::create_dir_all(&p).unwrap();
                Dir(p)
            }
            pub fn path(&self) -> &Path {
                &self.0
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn missing_config_is_empty() {
        let d = home_with(&[]);
        assert!(hosts(d.path()).is_empty());
    }

    #[test]
    fn a_directory_instead_of_a_file_is_empty() {
        let d = home_with(&[(".ssh/config/x", "Host a")]);
        assert!(hosts(d.path()).is_empty());
    }

    #[test]
    fn lists_hosts_several_per_line_and_leaves_patterns_out() {
        let d = home_with(&[(
            ".ssh/config",
            "# comment\nHost box other  # trailing\n  HostName 10.0.0.1\n  IdentityFile ~/.ssh/secret\n\
             host=lower\nHOST *.example.com !bad dev? \"quoted\" box\nHost *\n",
        )]);
        assert_eq!(hosts(d.path()), ["box", "other", "lower", "quoted"]);
    }

    #[test]
    fn match_blocks_are_ignored() {
        let d = home_with(&[(
            ".ssh/config",
            "Match host foo exec \"true\"\n  Host inner\nHost real\n",
        )]);
        // `Host` under a `Match` line is a `Host` line of the file; only the `Match` operands are
        // never read as names.
        let found = hosts(d.path());
        assert!(!found.contains(&"foo".to_string()));
        assert!(found.contains(&"real".to_string()));
    }

    #[test]
    fn follows_includes_relative_to_ssh_dir_with_wildcards() {
        let d = home_with(&[
            (".ssh/config", "Include conf.d/*.conf extra\nHost main\n"),
            (".ssh/conf.d/a.conf", "Host a1 a2\nInclude nested\n"),
            (".ssh/conf.d/b.conf", "Host b1\n"),
            (".ssh/conf.d/skip.txt", "Host skipped\n"),
            (".ssh/nested", "Host deep\n"),
            (".ssh/extra", "Host extra\n"),
        ]);
        assert_eq!(hosts(d.path()), ["a1", "a2", "deep", "b1", "extra", "main"]);
    }

    #[test]
    fn include_loops_end() {
        let d = home_with(&[(".ssh/config", "Include config\nHost one\n")]);
        assert_eq!(hosts(d.path()), ["one"]);
    }

    #[test]
    fn glob_matches() {
        assert!(glob("*.conf", "a.conf"));
        assert!(!glob("*.conf", "a.txt"));
        assert!(glob("a?c", "abc"));
    }
}
