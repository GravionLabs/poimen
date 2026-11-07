//! poimen: flags for `CreatePseudoConsole`, kept free of Windows types so the selection and the
//! fallback order can be unit tested on any host.

pub const INHERIT_CURSOR: u32 = 0x1;
pub const RESIZE_QUIRK: u32 = 0x2;
pub const WIN32_INPUT_MODE: u32 = 0x4;
pub const PASSTHROUGH_MODE: u32 = 0x8;

/// The flags upstream portable-pty 0.9 uses.
pub const UPSTREAM: u32 = INHERIT_CURSOR | RESIZE_QUIRK | WIN32_INPUT_MODE;

/// Flag sets to try in order: pass-through first (Windows 11 / recent Windows 10 conhost; VT input
/// such as mouse reports is not re-encoded), then the upstream flags for systems that reject
/// `PASSTHROUGH_MODE`.
pub fn attempts() -> [u32; 2] {
    [UPSTREAM | PASSTHROUGH_MODE, UPSTREAM]
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
}
