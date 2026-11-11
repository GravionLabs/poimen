//! Wire types of the host protocol between a server and a `poimen-pty-host` (ADR 0009,
//! `docs/protocol.md`, "Host protocol").
//!
//! One host owns one terminal. The server starts it with a [`HostStart`] line on stdin; the host
//! answers on stdout with [`HostMessage::Hello`] (or [`HostMessage::Error`]) and then serves one
//! client at a time on its endpoint: NDJSON, [`ClientMessage`]s in, [`HostMessage`]s out.
//!
//! The protocol changes only in a backwards compatible way (`HOST_VERSION` is reported in
//! `hello`, never required): unknown fields are ignored and unknown message types parse as
//! `Unknown`, so either side can skip them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The host protocol version this crate is written for, reported in `hello`.
pub const HOST_VERSION: u32 = 1;
/// Default size of the output ring of a host: 1 MiB of PTY output.
pub const DEFAULT_RING_BYTES: usize = 1024 * 1024;
/// The ring also keeps at most this many output records, so a replay stays bounded when the
/// child writes in tiny pieces.
pub const MAX_RING_RECORDS: usize = 16 * 1024;
/// Default time a host keeps running without any client: 24 h.
pub const DEFAULT_ORPHAN_TIMEOUT_MS: u64 = 24 * 60 * 60 * 1000;
/// Default time a host waits, after its child exited, for a client to collect the rest of the
/// output and the exit code.
pub const DEFAULT_EXIT_LINGER_MS: u64 = 30 * 1000;

/// What the host starts: the child's command line, directory, extra environment and size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnSpec {
    /// Program and arguments; never empty.
    pub argv: Vec<String>,
    pub cwd: String,
    /// Set in the child on top of the host's own environment.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    pub cols: u16,
    pub rows: u16,
}

/// The one line a server writes to the host's stdin to start it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostStart {
    /// Socket path (Unix) or the path of the pipe's marker file (Windows), as for the server.
    pub endpoint: PathBuf,
    pub spawn: SpawnSpec,
    /// The host kills its child and ends after this long without any client.
    #[serde(default = "default_orphan_timeout")]
    pub orphan_timeout_ms: u64,
    #[serde(default = "default_ring_bytes")]
    pub ring_bytes: usize,
    #[serde(default = "default_exit_linger")]
    pub exit_linger_ms: u64,
}

fn default_orphan_timeout() -> u64 {
    DEFAULT_ORPHAN_TIMEOUT_MS
}

fn default_ring_bytes() -> usize {
    DEFAULT_RING_BYTES
}

fn default_exit_linger() -> u64 {
    DEFAULT_EXIT_LINGER_MS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

/// A line from the host (on stdout at start, then on the endpoint).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    /// First line of every connection (and the host's stdout once it runs).
    Hello {
        host_version: u32,
        /// The host binary's version.
        version: String,
        pid: u32,
        child_pid: Option<u32>,
        size: Size,
        endpoint: PathBuf,
        /// The `seq` of the latest output record (0: no output yet).
        seq: u64,
    },
    /// PTY output, unchanged (base64). `seq` is strictly increasing per host, never reset.
    Output {
        seq: u64,
        bytes: String,
    },
    /// Records `from..=to` are no longer in the ring.
    Gap {
        from: u64,
        to: u64,
    },
    /// The child ended (after all its output); the host ends after it.
    Exit {
        code: Option<u32>,
    },
    /// Another client connected; this connection ends.
    Replaced,
    Error {
        message: String,
    },
    /// A type of a later host version.
    #[serde(other)]
    Unknown,
}

/// A line from the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Start the output stream after record `since` (0: the whole ring).
    Attach {
        #[serde(default)]
        since: u64,
    },
    /// Bytes for the PTY (base64).
    Input {
        bytes: String,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    /// End the child; the host sends `exit` and ends.
    Kill,
    /// A type of a later client.
    #[serde(other)]
    Unknown,
}

/// Parses one line of the host.
pub fn parse_host_line(line: &str) -> Result<HostMessage, serde_json::Error> {
    serde_json::from_str(line)
}

/// Parses one line of a client.
pub fn parse_client_line(line: &str) -> Result<ClientMessage, serde_json::Error> {
    serde_json::from_str(line)
}

/// One NDJSON line (with its `\n`).
pub fn to_line<T: Serialize>(message: &T) -> Vec<u8> {
    let mut line = serde_json::to_vec(message).expect("host messages serialize");
    line.push(b'\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_start_line_with_only_the_required_fields_gets_the_defaults() {
        let start: HostStart = serde_json::from_value(json!({
            "endpoint": "/run/u/poimen/hosts/t3.sock",
            "spawn": {"argv": ["sh"], "cwd": "/w", "cols": 80, "rows": 24}
        }))
        .unwrap();
        assert_eq!(start.orphan_timeout_ms, 86_400_000);
        assert_eq!(start.ring_bytes, 1_048_576);
        assert_eq!(start.exit_linger_ms, 30_000);
        assert!(start.spawn.env.is_empty());
        assert_eq!(start.spawn.argv, vec!["sh"]);
    }

    #[test]
    fn a_full_start_line_round_trips() {
        let start = HostStart {
            endpoint: PathBuf::from("/tmp/h.sock"),
            spawn: SpawnSpec {
                argv: vec!["sh".into(), "-c".into(), "echo hi".into()],
                cwd: "/tmp".into(),
                env: BTreeMap::from([("POIMEN_TERMINAL_ID".into(), "t1".into())]),
                cols: 100,
                rows: 30,
            },
            orphan_timeout_ms: 5,
            ring_bytes: 64,
            exit_linger_ms: 7,
        };
        let line = serde_json::to_string(&start).unwrap();
        assert_eq!(serde_json::from_str::<HostStart>(&line).unwrap(), start);
    }

    #[test]
    fn a_start_line_without_a_spawn_is_refused() {
        assert!(serde_json::from_str::<HostStart>(r#"{"endpoint":"/x"}"#).is_err());
    }

    #[test]
    fn hello_has_the_documented_shape() {
        let hello = HostMessage::Hello {
            host_version: 1,
            version: "0.16.0".into(),
            pid: 10,
            child_pid: Some(11),
            size: Size { cols: 80, rows: 24 },
            endpoint: PathBuf::from("/tmp/h.sock"),
            seq: 0,
        };
        assert_eq!(
            serde_json::to_value(&hello).unwrap(),
            json!({
                "type": "hello", "host_version": 1, "version": "0.16.0", "pid": 10,
                "child_pid": 11, "size": {"cols": 80, "rows": 24}, "endpoint": "/tmp/h.sock",
                "seq": 0
            })
        );
    }

    #[test]
    fn host_lines_parse() {
        assert_eq!(
            parse_host_line(r#"{"type":"output","seq":3,"bytes":"aGk="}"#).unwrap(),
            HostMessage::Output {
                seq: 3,
                bytes: "aGk=".into()
            }
        );
        assert_eq!(
            parse_host_line(r#"{"type":"gap","from":1,"to":9}"#).unwrap(),
            HostMessage::Gap { from: 1, to: 9 }
        );
        assert_eq!(
            parse_host_line(r#"{"type":"exit","code":null}"#).unwrap(),
            HostMessage::Exit { code: None }
        );
        assert_eq!(
            parse_host_line(r#"{"type":"replaced"}"#).unwrap(),
            HostMessage::Replaced
        );
    }

    #[test]
    fn unknown_types_and_fields_of_a_later_version_are_tolerated() {
        assert_eq!(
            parse_host_line(r#"{"type":"title","text":"x"}"#).unwrap(),
            HostMessage::Unknown
        );
        assert_eq!(
            parse_host_line(r#"{"type":"exit","code":3,"signal":"HUP"}"#).unwrap(),
            HostMessage::Exit { code: Some(3) }
        );
        assert_eq!(
            parse_client_line(r#"{"type":"pause"}"#).unwrap(),
            ClientMessage::Unknown
        );
    }

    #[test]
    fn client_lines_parse() {
        assert_eq!(
            parse_client_line(r#"{"type":"attach"}"#).unwrap(),
            ClientMessage::Attach { since: 0 }
        );
        assert_eq!(
            parse_client_line(r#"{"type":"attach","since":41}"#).unwrap(),
            ClientMessage::Attach { since: 41 }
        );
        assert_eq!(
            parse_client_line(r#"{"type":"resize","cols":120,"rows":40}"#).unwrap(),
            ClientMessage::Resize {
                cols: 120,
                rows: 40
            }
        );
        assert_eq!(
            parse_client_line(r#"{"type":"kill"}"#).unwrap(),
            ClientMessage::Kill
        );
        assert_eq!(
            parse_client_line(r#"{"type":"input","bytes":"bHM="}"#).unwrap(),
            ClientMessage::Input {
                bytes: "bHM=".into()
            }
        );
    }

    #[test]
    fn malformed_client_lines_are_errors() {
        assert!(parse_client_line("not json").is_err());
        assert!(parse_client_line(r#"{"since":1}"#).is_err());
        assert!(parse_client_line(r#"{"type":"resize","cols":"wide","rows":1}"#).is_err());
    }

    #[test]
    fn a_line_ends_with_a_newline() {
        assert_eq!(
            to_line(&ClientMessage::Kill),
            b"{\"type\":\"kill\"}\n".to_vec()
        );
    }
}
