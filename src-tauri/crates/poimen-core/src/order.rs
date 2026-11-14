//! Ordering shared by the clients: the alphabetical order of `sidebar.sort: "name"`.

use std::cmp::Ordering;

/// Compares two texts the way the sidebar sorts by name: case-insensitive, runs of ASCII digits as
/// numbers (`#9` before `#12`).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (x, y) = (digit_run(&mut a), digit_run(&mut b));
                let (xt, yt) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let ord = xt.len().cmp(&yt.len()).then_with(|| xt.cmp(yt));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                a.next();
                b.next();
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

fn digit_run(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut run = String::new();
    while let Some(c) = it.next_if(char::is_ascii_digit) {
        run.push(c);
    }
    run
}

/// Order of two spaces by name; equal names fall back to the ids (naturally, so `s2` before `s10`).
pub fn compare_space_names(a_name: &str, a_id: &str, b_name: &str, b_id: &str) -> Ordering {
    natural_cmp(a_name, b_name).then_with(|| natural_cmp(a_id, b_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(names: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = names.iter().map(|s| (*s).to_owned()).collect();
        v.sort_by(|a, b| natural_cmp(a, b));
        v
    }

    #[test]
    fn case_is_ignored() {
        assert_eq!(
            sorted(&["beta", "Alpha", "alpha2"]),
            ["Alpha", "alpha2", "beta"]
        );
        assert_eq!(natural_cmp("ABC", "abc"), Ordering::Equal);
    }

    #[test]
    fn digit_runs_compare_as_numbers() {
        assert_eq!(
            sorted(&["#12 x", "#9 y", "#100 z"]),
            ["#9 y", "#12 x", "#100 z"]
        );
        assert_eq!(natural_cmp("a007", "a7"), Ordering::Equal);
        assert_eq!(natural_cmp("a2b", "a10a"), Ordering::Less);
    }

    #[test]
    fn prefix_sorts_first_and_empty_is_least() {
        assert_eq!(natural_cmp("ab", "abc"), Ordering::Less);
        assert_eq!(natural_cmp("", "a"), Ordering::Less);
        assert_eq!(natural_cmp("", ""), Ordering::Equal);
    }

    #[test]
    fn very_long_digit_runs_do_not_overflow() {
        let big = "9".repeat(60);
        let bigger = format!("1{}", "0".repeat(60));
        assert_eq!(natural_cmp(&big, &bigger), Ordering::Less);
    }

    #[test]
    fn equal_names_tie_by_id_naturally() {
        assert_eq!(compare_space_names("x", "s2", "X", "s10"), Ordering::Less);
        assert_eq!(
            compare_space_names("x", "s10", "x", "s2"),
            Ordering::Greater
        );
        assert_eq!(compare_space_names("a", "s9", "b", "s1"), Ordering::Less);
    }
}
