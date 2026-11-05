//! The local socket between `poimen-server` and its clients, through `interprocess`: a Unix
//! domain socket at the socket path on Linux and macOS, a named pipe `\\.\pipe\<socket path>` on
//! Windows with a marker file (`<pid>:<nanos>`) at the socket path, so "is a server running?"
//! looks the same on both (`docs/protocol.md`, Transport).
//!
//! Named pipes have neither I/O timeouts nor `shutdown`: on Windows [`Conn::set_read_timeout`],
//! [`Conn::set_write_timeout`] and [`Conn::shutdown`] have no effect ([`TIMEOUTS_SUPPORTED`]), and
//! callers that need a bound wait on a helper thread.

use std::fmt;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Whether [`Conn::set_read_timeout`] and [`Conn::set_write_timeout`] take effect.
pub const TIMEOUTS_SUPPORTED: bool = cfg!(unix);

/// `\\.\pipe\<path>`: the named pipe a server at `path` listens on (Windows). The same scheme as
/// herdr's.
pub fn pipe_name(path: &Path) -> String {
    format!(r"\\.\pipe\{}", path.display())
}

/// The content of the marker file at the socket path on Windows: `<pid>:<unix nanos>`.
pub fn marker(pid: u32, nanos: u128) -> String {
    format!("{pid}:{nanos}")
}

/// Whether `text` is a marker written by [`marker`].
pub fn is_marker(text: &str) -> bool {
    text.trim()
        .split_once(':')
        .is_some_and(|(pid, nanos)| pid.parse::<u32>().is_ok() && nanos.parse::<u128>().is_ok())
}

/// What lies at a socket path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Missing,
    /// A server's entry: the socket file (Unix) or the marker file (Windows). It may be stale.
    Socket,
    /// Something that is not ours; never removed.
    Other,
}

/// What lies at `path`, without following a symlink.
pub fn inspect(path: &Path) -> io::Result<Entry> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Entry::Missing),
        Err(e) => return Err(e),
    };
    Ok(if sys::is_entry(path, &meta) {
        Entry::Socket
    } else {
        Entry::Other
    })
}

/// Whether a server answers a connect at `path` (no request is sent).
pub fn is_listening(path: &Path) -> bool {
    Conn::connect(path).is_ok()
}

/// Whether the error means the peer closed the connection.
pub fn is_closed_error(e: &io::Error) -> bool {
    use io::ErrorKind::*;
    matches!(
        e.kind(),
        BrokenPipe | ConnectionAborted | ConnectionReset | NotConnected | UnexpectedEof | WriteZero
    ) || (cfg!(windows)
        // ERROR_INVALID_HANDLE, ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_NOT_CONNECTED
        && matches!(e.raw_os_error(), Some(6 | 109 | 232 | 233)))
}

/// A bound listener. Unix: the socket at the path, mode 0600, left in place when dropped.
/// Windows: the named pipe of the path, readable by SYSTEM and its owner only, and the marker file.
pub struct Listener(sys::RawListener);

impl fmt::Debug for Listener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Listener").finish_non_exhaustive()
    }
}

impl Listener {
    /// Binds `path`. Fails with [`io::ErrorKind::AddrInUse`] when it is taken (Unix: a socket
    /// file exists; Windows: another server holds the pipe).
    pub fn bind(path: &Path) -> io::Result<Self> {
        sys::bind(path).map(Self)
    }

    /// Waits for the next client.
    pub fn accept(&self) -> io::Result<Conn> {
        sys::accept(&self.0).map(|raw| Conn(Arc::new(raw)))
    }

    /// The clients, one after the other, for ever.
    pub fn incoming(&self) -> impl Iterator<Item = io::Result<Conn>> + '_ {
        std::iter::from_fn(move || Some(self.accept()))
    }
}

/// One connection. A clone is another handle on the same connection: one thread may read while
/// another writes.
#[derive(Clone, Debug)]
pub struct Conn(Arc<sys::Raw>);

impl Conn {
    /// Connects to the server at `path`. Nothing there is [`io::ErrorKind::NotFound`] (Windows,
    /// and Unix without a socket file) or [`io::ErrorKind::ConnectionRefused`] (Unix, stale file).
    pub fn connect(path: &Path) -> io::Result<Self> {
        sys::connect(path).map(|raw| Conn(Arc::new(raw)))
    }

    /// Bounds blocking reads (`None`: for ever). No effect on Windows.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        sys::set_read_timeout(&self.0, timeout)
    }

    /// Bounds blocking writes (`None`: for ever). No effect on Windows.
    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        sys::set_write_timeout(&self.0, timeout)
    }

    /// Shuts both directions down, which wakes a reader blocked on another handle. No effect on
    /// Windows, where the connection ends when the peer drops it.
    pub fn shutdown(&self) -> io::Result<()> {
        sys::shutdown(&self.0)
    }
}

impl Read for &Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self.0).read(buf)
    }
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self).read(buf)
    }
}

impl Write for &Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self.0).write(buf)
    }

    /// Nothing to flush: writes go straight to the socket. (A pipe's own flush would block until
    /// the peer has read everything.)
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(unix)]
mod sys {
    use std::io;
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::time::Duration;

    use interprocess::local_socket::traits::{Listener as _, Stream as _};
    use interprocess::local_socket::{GenericFilePath, ListenerOptions, Stream, ToFsName};

    pub type Raw = Stream;
    pub type RawListener = interprocess::local_socket::Listener;

    pub fn is_entry(_: &Path, meta: &std::fs::Metadata) -> bool {
        std::os::unix::fs::FileTypeExt::is_socket(&meta.file_type())
    }

    pub fn bind(path: &Path) -> io::Result<RawListener> {
        use std::os::unix::fs::PermissionsExt;
        let listener = ListenerOptions::new()
            .name(path.to_fs_name::<GenericFilePath>()?)
            // The socket file stays when the listener goes, as with std's UnixListener: only the
            // next server (holding the lock) judges it stale.
            .reclaim_name(false)
            .create_sync()?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(listener)
    }

    pub fn accept(listener: &RawListener) -> io::Result<Raw> {
        listener.accept()
    }

    pub fn connect(path: &Path) -> io::Result<Raw> {
        Stream::connect(path.to_fs_name::<GenericFilePath>()?)
    }

    fn inner(raw: &Raw) -> &UnixStream {
        match raw {
            Stream::UdSocket(s) => s.inner(),
        }
    }

    pub fn set_read_timeout(raw: &Raw, timeout: Option<Duration>) -> io::Result<()> {
        inner(raw).set_read_timeout(timeout)
    }

    pub fn set_write_timeout(raw: &Raw, timeout: Option<Duration>) -> io::Result<()> {
        inner(raw).set_write_timeout(timeout)
    }

    pub fn shutdown(raw: &Raw) -> io::Result<()> {
        inner(raw).shutdown(std::net::Shutdown::Both)
    }
}

#[cfg(windows)]
mod sys {
    use std::io;
    use std::path::Path;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use interprocess::ConnectWaitMode;
    use interprocess::os::windows::named_pipe::{
        DuplexPipeStream, PipeListener, PipeListenerOptions, pipe_mode::Bytes,
    };
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use widestring::U16CString;

    use super::{is_marker, marker, pipe_name};

    pub type Raw = DuplexPipeStream<Bytes>;
    pub type RawListener = PipeListener<Bytes, Bytes>;

    /// Protected DACL: full access for SYSTEM and the owner (the user who started the server).
    const SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;OW)";
    /// Records reach 32 KiB; the default 512-byte buffers would make every write wait for the
    /// reader.
    const BUFFER: u32 = 64 * 1024;
    /// How long a connect waits for a free pipe instance (the listener creates the next one right
    /// after a client connected).
    const CONNECT_WAIT: Duration = Duration::from_secs(2);

    pub fn is_entry(path: &Path, meta: &std::fs::Metadata) -> bool {
        meta.is_file()
            && meta.len() < 64
            && std::fs::read_to_string(path).is_ok_and(|text| is_marker(&text))
    }

    pub fn bind(path: &Path) -> io::Result<RawListener> {
        let sddl = U16CString::from_str(SDDL)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let descriptor = SecurityDescriptor::deserialize(&sddl)?;
        let listener = PipeListenerOptions::new()
            .path(pipe_name(path))
            .security_descriptor(Some(descriptor))
            .input_buffer_size_hint(BUFFER)
            .output_buffer_size_hint(BUFFER)
            .create_duplex::<Bytes>()
            .map_err(|e| {
                // The first instance is created with FILE_FLAG_FIRST_PIPE_INSTANCE: a pipe that
                // exists already is refused with ERROR_ACCESS_DENIED.
                if e.kind() == io::ErrorKind::PermissionDenied {
                    io::Error::new(io::ErrorKind::AddrInUse, e)
                } else {
                    e
                }
            })?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        std::fs::write(path, marker(std::process::id(), nanos))?;
        Ok(listener)
    }

    pub fn accept(listener: &RawListener) -> io::Result<Raw> {
        listener.accept()
    }

    pub fn connect(path: &Path) -> io::Result<Raw> {
        DuplexPipeStream::<Bytes>::connect_by_path_with_wait_mode(
            pipe_name(path),
            ConnectWaitMode::Timeout(CONNECT_WAIT),
        )
    }

    pub fn set_read_timeout(_: &Raw, _: Option<Duration>) -> io::Result<()> {
        Ok(())
    }

    pub fn set_write_timeout(_: &Raw, _: Option<Duration>) -> io::Result<()> {
        Ok(())
    }

    pub fn shutdown(_: &Raw) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::path::PathBuf;
    use std::thread;

    fn temp_socket(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "poimen-transport-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("s.sock")
    }

    #[test]
    fn pipe_name_prefixes_the_path() {
        assert_eq!(
            pipe_name(Path::new(r"C:\Users\u\AppData\Local\poimen\server.sock")),
            r"\\.\pipe\C:\Users\u\AppData\Local\poimen\server.sock"
        );
    }

    #[test]
    fn a_marker_holds_pid_and_nanos() {
        let text = marker(42, 1_700_000_000_000_000_000);
        assert_eq!(text, "42:1700000000000000000");
        assert!(is_marker(&text));
        assert!(!is_marker("hello"));
        assert!(!is_marker("1:x"));
        assert!(!is_marker(""));
    }

    #[test]
    fn bind_connect_round_trip() {
        let path = temp_socket("round-trip");
        let listener = Listener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let conn = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&conn).read_line(&mut line).unwrap();
            (&conn).write_all(b"pong\n").unwrap();
            line
        });
        let conn = Conn::connect(&path).unwrap();
        (&conn).write_all(b"ping\n").unwrap();
        let mut answer = String::new();
        BufReader::new(&conn).read_line(&mut answer).unwrap();
        assert_eq!(answer, "pong\n");
        assert_eq!(server.join().unwrap(), "ping\n");
    }

    #[test]
    fn a_clone_writes_while_another_handle_reads() {
        let path = temp_socket("duplex");
        let listener = Listener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let conn = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&conn).read_line(&mut line).unwrap();
            (&conn).write_all(line.as_bytes()).unwrap();
        });
        let conn = Conn::connect(&path).unwrap();
        let reader = conn.clone();
        // The reader blocks before anything was written.
        let read = thread::spawn(move || {
            let mut line = String::new();
            BufReader::new(&reader).read_line(&mut line).unwrap();
            line
        });
        thread::sleep(Duration::from_millis(50));
        (&conn).write_all(b"echo\n").unwrap();
        assert_eq!(read.join().unwrap(), "echo\n");
        server.join().unwrap();
    }

    #[test]
    fn a_large_record_passes_while_the_peer_reads() {
        let path = temp_socket("large");
        let listener = Listener::bind(&path).unwrap();
        let record = format!("{}\n", "x".repeat(32 * 1024));
        let sent = record.clone();
        let server = thread::spawn(move || {
            let conn = listener.accept().unwrap();
            (&conn).write_all(sent.as_bytes()).unwrap();
        });
        let conn = Conn::connect(&path).unwrap();
        let mut line = String::new();
        BufReader::new(&conn).read_line(&mut line).unwrap();
        assert_eq!(line, record);
        server.join().unwrap();
    }

    #[test]
    fn connect_to_nothing_fails_as_not_running() {
        let err = Conn::connect(&temp_socket("nothing")).unwrap_err();
        assert!(
            matches!(
                err.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ),
            "{err:?}"
        );
        assert!(!is_listening(&temp_socket("nothing")));
    }

    #[test]
    fn a_second_bind_fails_while_the_first_lives() {
        let path = temp_socket("second");
        let _first = Listener::bind(&path).unwrap();
        assert!(is_listening(&path));
        let err = Listener::bind(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AddrInUse, "{err:?}");
    }

    #[test]
    fn inspect_classifies_missing_socket_and_other() {
        let path = temp_socket("inspect");
        assert_eq!(inspect(&path).unwrap(), Entry::Missing);
        let listener = Listener::bind(&path).unwrap();
        assert_eq!(inspect(&path).unwrap(), Entry::Socket);
        drop(listener);
        // A dropped listener leaves its entry behind, stale.
        assert_eq!(inspect(&path).unwrap(), Entry::Socket);
        assert!(!is_listening(&path));
        let other = path.with_file_name("other");
        std::fs::write(&other, "not a socket").unwrap();
        assert_eq!(inspect(&other).unwrap(), Entry::Other);
        std::fs::create_dir(path.with_file_name("dir")).unwrap();
        assert_eq!(inspect(&path.with_file_name("dir")).unwrap(), Entry::Other);
    }

    #[test]
    fn closed_errors_are_recognised() {
        assert!(is_closed_error(&io::ErrorKind::BrokenPipe.into()));
        assert!(is_closed_error(&io::ErrorKind::UnexpectedEof.into()));
        assert!(!is_closed_error(&io::ErrorKind::PermissionDenied.into()));
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_wakes_a_blocked_reader() {
        let path = temp_socket("shutdown");
        let listener = Listener::bind(&path).unwrap();
        let conn = Conn::connect(&path).unwrap();
        let _server = listener.accept().unwrap();
        let reader = conn.clone();
        let read = thread::spawn(move || {
            let mut line = String::new();
            BufReader::new(&reader).read_line(&mut line)
        });
        thread::sleep(Duration::from_millis(50));
        conn.shutdown().unwrap();
        assert_eq!(read.join().unwrap().unwrap(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn the_socket_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_socket("mode");
        let _listener = Listener::bind(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn read_timeouts_apply_on_unix() {
        let path = temp_socket("timeout");
        let listener = Listener::bind(&path).unwrap();
        let conn = Conn::connect(&path).unwrap();
        let _server = listener.accept().unwrap();
        conn.set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let err = (&conn).read(&mut [0; 1]).unwrap_err();
        assert!(matches!(
            err.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ));
    }
}
