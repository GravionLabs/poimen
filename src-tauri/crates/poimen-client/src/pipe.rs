//! Connections that are not the local socket: a [`Connector`] opens a [`Pipe`] (a read half and a
//! write half, for example the stdout and stdin of `ssh <target> poimen-server stdio`), and
//! [`crate::Client::with_connector`] runs every request, subscription and attachment over it.
//!
//! A pipe has no I/O timeouts; requests are bounded by a thread of their own, as on a named pipe.

use std::fmt;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

/// Opens one connection per call, like a connect on the local socket.
pub trait Connector: Send + Sync {
    fn connect(&self) -> io::Result<Pipe>;
}

type CloseHook = Box<dyn FnOnce() + Send>;

struct Inner {
    reader: Mutex<Box<dyn Read + Send>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    on_close: Mutex<Option<CloseHook>>,
}

impl Inner {
    fn close(&self) {
        lock(&self.writer).take();
        if let Some(hook) = lock(&self.on_close).take() {
            hook();
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.close();
    }
}

/// One connection over a read half and a write half. A clone is another handle on the same
/// connection: one thread may read while another writes. The connection closes when the last
/// handle is dropped or [`Pipe::close`] is called; then the close hook runs once (it ends the
/// process behind the pipe, which wakes a blocked reader).
#[derive(Clone)]
pub struct Pipe(Arc<Inner>);

impl fmt::Debug for Pipe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Pipe")
    }
}

impl Pipe {
    pub fn new(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        on_close: impl FnOnce() + Send + 'static,
    ) -> Self {
        Pipe(Arc::new(Inner {
            reader: Mutex::new(Box::new(reader)),
            writer: Mutex::new(Some(Box::new(writer))),
            on_close: Mutex::new(Some(Box::new(on_close))),
        }))
    }

    /// Closes the write half and runs the close hook; later writes fail.
    pub fn close(&self) {
        self.0.close();
    }

    /// A handle that does not keep the connection open.
    pub fn downgrade(&self) -> WeakPipe {
        WeakPipe(Arc::downgrade(&self.0))
    }
}

/// A [`Pipe`] that may already be gone (see [`Pipe::downgrade`]).
#[derive(Clone)]
pub struct WeakPipe(Weak<Inner>);

impl WeakPipe {
    pub fn upgrade(&self) -> Option<Pipe> {
        self.0.upgrade().map(Pipe)
    }
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "the connection is closed")
}

impl Read for &Pipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        lock(&self.0.reader).read(buf)
    }
}

impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self).read(buf)
    }
}

impl Write for &Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match lock(&self.0.writer).as_mut() {
            Some(w) => w.write(buf),
            None => Err(closed()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match lock(&self.0.writer).as_mut() {
            Some(w) => w.flush(),
            None => Err(closed()),
        }
    }
}

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&*self).flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn close_runs_the_hook_once_and_fails_later_writes() {
        let closes = Arc::new(AtomicU32::new(0));
        let counter = closes.clone();
        let pipe = Pipe::new(io::empty(), Vec::new(), move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let other = pipe.clone();
        (&pipe).write_all(b"x").unwrap();
        pipe.close();
        assert_eq!(
            (&other).write_all(b"y").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        drop(pipe);
        drop(other);
        assert_eq!(closes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_weak_handle_closes_a_live_pipe_and_is_empty_after_the_last_drop() {
        let pipe = Pipe::new(io::empty(), io::sink(), || {});
        let weak = pipe.downgrade();
        weak.upgrade().unwrap().close();
        assert!((&pipe).write_all(b"x").is_err());
        drop(pipe);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn dropping_the_last_handle_closes_the_pipe() {
        let closes = Arc::new(AtomicU32::new(0));
        let counter = closes.clone();
        let pipe = Pipe::new(io::empty(), io::sink(), move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let clone = pipe.clone();
        drop(pipe);
        assert_eq!(closes.load(Ordering::SeqCst), 0);
        drop(clone);
        assert_eq!(closes.load(Ordering::SeqCst), 1);
    }
}
