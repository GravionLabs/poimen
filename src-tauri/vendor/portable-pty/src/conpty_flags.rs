//! poimen: flags for `CreatePseudoConsole`, kept free of Windows types so the selection and the
//! fallback order can be unit tested on any host.

use std::sync::atomic::{AtomicBool, Ordering};

pub const INHERIT_CURSOR: u32 = 0x1;
pub const RESIZE_QUIRK: u32 = 0x2;
pub const WIN32_INPUT_MODE: u32 = 0x4;
pub const PASSTHROUGH_MODE: u32 = 0x8;

/// The flags upstream portable-pty 0.9 uses.
pub const UPSTREAM: u32 = INHERIT_CURSOR | RESIZE_QUIRK | WIN32_INPUT_MODE;

static INHERIT: AtomicBool = AtomicBool::new(true);

/// Whether pseudo consoles created from now on get `INHERIT_CURSOR` (default: yes, as upstream).
///
/// With the flag conhost asks for the cursor position (DSR `ESC [ 6 n`) when it starts and holds
/// the child back until the reply arrives: no output, and a child that cannot run never exits. A
/// process with nobody to answer (`poimen-pty-host` while no client is connected) turns it off;
/// the console then starts with the cursor at the top left.
pub fn set_inherit_cursor(inherit: bool) {
    INHERIT.store(inherit, Ordering::Relaxed);
}

/// Flag sets to try in order: pass-through first (Windows 11 / recent Windows 10 conhost; VT input
/// such as mouse reports is not re-encoded), then the upstream flags for systems that reject
/// `PASSTHROUGH_MODE`. Both without `INHERIT_CURSOR` once [`set_inherit_cursor`] turned it off.
pub fn attempts() -> [u32; 2] {
    attempts_for(INHERIT.load(Ordering::Relaxed))
}

fn attempts_for(inherit: bool) -> [u32; 2] {
    let base = if inherit {
        UPSTREAM
    } else {
        UPSTREAM & !INHERIT_CURSOR
    };
    [base | PASSTHROUGH_MODE, base]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_is_tried_first_and_upstream_is_the_fallback() {
        let a = attempts();
        assert_eq!(a[0], 0x1 | 0x2 | 0x4 | 0x8);
        assert_eq!(a[1], 0x7);
    }

    #[test]
    fn without_inherit_cursor_both_attempts_drop_only_that_flag() {
        assert_eq!(attempts_for(false), [0x2 | 0x4 | 0x8, 0x2 | 0x4]);
        assert_eq!(attempts_for(true), [0xf, 0x7]);
    }
}
