//! The release channel of a build, fixed at build time by `POIMEN_CHANNEL`.

use serde::{Deserialize, Serialize};

/// The release channel a binary is built for, fixed at build time by `POIMEN_CHANNEL` (`rc` for
/// the pre-release builds of `release.yml`, unset or `stable` otherwise). Every path a stable
/// and an rc installation could share is named per channel, so the two run side by side, each
/// with its own server, settings and hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Rc,
}

impl Channel {
    pub const ALL: [Channel; 2] = [Channel::Stable, Channel::Rc];

    /// The channel of a `POIMEN_CHANNEL` value: unset, empty or `stable` is stable, `rc` is rc, else `None`.
    pub const fn parse(value: Option<&str>) -> Option<Channel> {
        match value {
            None => Some(Channel::Stable),
            Some(v) if v.is_empty() || const_eq(v, "stable") => Some(Channel::Stable),
            Some(v) if const_eq(v, "rc") => Some(Channel::Rc),
            Some(_) => None,
        }
    }

    /// `stable` or `rc`.
    pub const fn name(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Rc => "rc",
        }
    }

    /// What names of this channel end with: nothing on stable, `-rc` on rc.
    pub const fn suffix(self) -> &'static str {
        match self {
            Channel::Stable => "",
            Channel::Rc => "-rc",
        }
    }

    /// `base` with the channel suffix: `poimen` stays `poimen` on stable and is `poimen-rc` on rc.
    pub fn named(self, base: &str) -> String {
        format!("{base}{}", self.suffix())
    }

    /// The file name of this channel's `poimen-server` binary (`poimen-server-rc` on rc; `.exe` on Windows).
    pub fn server_program(self, windows: bool) -> String {
        let name = self.named("poimen-server");
        if windows { format!("{name}.exe") } else { name }
    }

    /// The file name of this channel's `poimen-pty-host` binary, next to the server (`-rc` on rc; `.exe` on Windows).
    pub fn host_program(self, windows: bool) -> String {
        let name = self.named("poimen-pty-host");
        if windows { format!("{name}.exe") } else { name }
    }
}

const fn const_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}
