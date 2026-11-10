//! Keyboard input of `poimen attach`: the detach key and UTF-8 joining.

/// Default prefix key, `Ctrl+B` (ADR 0006).
pub const DEFAULT_PREFIX: u8 = 0x02;

/// Prefix then `d` detaches; the prefix twice sends the prefix; any other key after the prefix
/// goes through unchanged together with the prefix.
#[derive(Debug)]
pub struct KeyFilter {
    prefix: u8,
    pending: bool,
}

/// What a chunk of input turned into.
#[derive(Debug, PartialEq, Eq)]
pub struct Filtered {
    /// Bytes for the terminal.
    pub forward: Vec<u8>,
    /// The detach sequence was typed; later bytes of the chunk are dropped.
    pub detach: bool,
}

impl KeyFilter {
    pub fn new(prefix: u8) -> Self {
        Self {
            prefix,
            pending: false,
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Filtered {
        let mut forward = Vec::with_capacity(chunk.len());
        for &b in chunk {
            if self.pending {
                self.pending = false;
                if b == b'd' {
                    return Filtered {
                        forward,
                        detach: true,
                    };
                }
                if b != self.prefix {
                    forward.push(self.prefix);
                }
                forward.push(b);
            } else if b == self.prefix {
                self.pending = true;
            } else {
                forward.push(b);
            }
        }
        Filtered {
            forward,
            detach: false,
        }
    }
}

/// Joins raw input bytes into strings without splitting a multi-byte character that arrives in
/// two reads; invalid bytes become U+FFFD.
#[derive(Debug, Default)]
pub struct Utf8Joiner {
    tail: Vec<u8>,
}

impl Utf8Joiner {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.tail.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.tail) {
                Ok(s) => {
                    out.push_str(s);
                    self.tail.clear();
                    return out;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    out.push_str(std::str::from_utf8(&self.tail[..valid]).unwrap_or_default());
                    match e.error_len() {
                        None => {
                            self.tail.drain(..valid);
                            return out;
                        }
                        Some(n) => {
                            out.push('\u{fffd}');
                            self.tail.drain(..valid + n);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_input_passes_through() {
        let mut f = KeyFilter::new(DEFAULT_PREFIX);
        assert_eq!(
            f.feed(b"ls\r"),
            Filtered {
                forward: b"ls\r".to_vec(),
                detach: false
            }
        );
    }

    #[test]
    fn prefix_d_detaches_and_drops_the_rest() {
        let mut f = KeyFilter::new(DEFAULT_PREFIX);
        assert_eq!(
            f.feed(b"ab\x02dzz"),
            Filtered {
                forward: b"ab".to_vec(),
                detach: true
            }
        );
    }

    #[test]
    fn prefix_across_two_chunks_detaches() {
        let mut f = KeyFilter::new(DEFAULT_PREFIX);
        assert!(!f.feed(b"\x02").detach);
        assert!(f.feed(b"d").detach);
    }

    #[test]
    fn prefix_twice_sends_one_prefix() {
        let mut f = KeyFilter::new(DEFAULT_PREFIX);
        assert_eq!(f.feed(b"\x02\x02").forward, vec![0x02]);
    }

    #[test]
    fn other_key_after_prefix_goes_through_with_it() {
        let mut f = KeyFilter::new(DEFAULT_PREFIX);
        assert_eq!(f.feed(b"\x02x").forward, vec![0x02, b'x']);
        assert!(!f.feed(b"d").detach);
    }

    #[test]
    fn joiner_keeps_a_split_character() {
        let mut j = Utf8Joiner::default();
        let bytes = "é".as_bytes();
        assert_eq!(j.push(&bytes[..1]), "");
        assert_eq!(j.push(&bytes[1..]), "é");
    }

    #[test]
    fn joiner_replaces_invalid_bytes() {
        let mut j = Utf8Joiner::default();
        assert_eq!(j.push(b"a\xffb"), "a\u{fffd}b");
    }
}
