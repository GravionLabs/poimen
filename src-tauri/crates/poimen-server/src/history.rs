//! The terminal history log (#973, ADR 0015): what a terminal printed and how it was resized,
//! on disk, so a later step can show it after a reboot.
//!
//! One file per terminal, `<history dir>/<terminal id>`: the 8 bytes [`MAGIC`], then records
//! `len: u32 LE | crc32: u32 LE | payload`, where the payload is a kind byte and its body
//! (`1` output bytes, `2` resize as `cols: u16 LE, rows: u16 LE`, `3` checkpoint, `4` the host
//! `seq` the output before it reaches) and the checksum covers the payload. A checkpoint (#1194)
//! is `seq: u64 LE, cols: u16 LE, rows: u16 LE` and the serialized grid (the bytes of
//! `Emulator::snapshot`): the state after every record before it in the file. Files are private (mode 0600, the directory 0700; an owner-only ACL on Windows).
//!
//! The server only queues a record ([`History::output`], [`History::resize`]): a worker thread
//! owns the files, so the PTY path never waits for the disk. A full queue drops the record and
//! logs it once; a write error is logged once per terminal. Neither stops the terminal.
//! Terminal output is on disk here: see the privacy rule in the ADR.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};

/// Start of every log file.
pub const MAGIC: &[u8; 8] = b"PMNHIST1";
/// Default size cap of one terminal's log.
pub const DEFAULT_PER_TERMINAL: u64 = 8 * 1024 * 1024;
/// Default size cap of all logs together.
pub const DEFAULT_TOTAL: u64 = 256 * 1024 * 1024;
/// Records queued for the worker before new ones are dropped.
const QUEUE: usize = 4096;
/// A longer payload is damage, not a record.
const MAX_PAYLOAD: u32 = 16 * 1024 * 1024;
const FRAME_HEAD: usize = 8;
const KIND_OUTPUT: u8 = 1;
const KIND_RESIZE: u8 = 2;
const KIND_CHECKPOINT: u8 = 3;
const KIND_SEQ: u8 = 4;
/// Kind byte, `seq`, `cols`, `rows` of a checkpoint payload.
const CHECKPOINT_HEAD: usize = 13;

/// One record of a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Output(Vec<u8>),
    Resize {
        cols: u16,
        rows: u16,
    },
    /// The host `seq` of the last output record logged so far (#1194).
    Seq(u64),
    Checkpoint(Checkpoint),
}

/// The grid of a terminal at one point of its log (#1194).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// The host `seq` of the last output record the grid has seen (0: unknown).
    pub seq: u64,
    pub cols: u16,
    pub rows: u16,
    /// The bytes that bring a fresh emulator of this size to the state.
    pub grid: Vec<u8>,
}

/// The size caps in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub per_terminal: u64,
    pub total: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            per_terminal: DEFAULT_PER_TERMINAL,
            total: DEFAULT_TOTAL,
        }
    }
}

enum Msg {
    Frame {
        id: String,
        frame: Vec<u8>,
    },
    /// Removes the log of `id`.
    Reset(String),
    Flush(SyncSender<()>),
}

/// The writing side: queues records for the worker thread.
pub struct History {
    tx: Option<SyncSender<Msg>>,
    log: PathBuf,
    dir: PathBuf,
    overflowed: AtomicBool,
}

impl History {
    /// Starts the worker for `dir` (created on the first write). `log` is the `server.log`
    /// the worker reports to.
    pub fn start(dir: PathBuf, limits: Limits, log: PathBuf) -> Self {
        let (tx, rx) = sync_channel::<Msg>(QUEUE);
        let mut worker = Worker::new(dir.clone(), limits, log.clone());
        let spawned = std::thread::Builder::new()
            .name("history".into())
            .spawn(move || {
                for msg in rx {
                    match msg {
                        Msg::Frame { id, frame } => worker.append(&id, &frame),
                        Msg::Reset(id) => worker.reset(&id),
                        Msg::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            });
        if spawned.is_err() {
            crate::log::line(&log, "history", "could not start the history writer");
            return Self::disabled();
        }
        Self {
            tx: Some(tx),
            log,
            dir,
            overflowed: AtomicBool::new(false),
        }
    }

    /// The file of the log of `terminal`.
    pub(crate) fn path(&self, terminal: &str) -> PathBuf {
        self.dir.join(terminal)
    }

    /// Writes nothing.
    pub fn disabled() -> Self {
        Self {
            tx: None,
            log: PathBuf::new(),
            dir: PathBuf::new(),
            overflowed: AtomicBool::new(false),
        }
    }

    /// Queues PTY output of `terminal`.
    pub fn output(&self, terminal: &str, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut payload = Vec::with_capacity(1 + bytes.len());
        payload.push(KIND_OUTPUT);
        payload.extend_from_slice(bytes);
        self.send(terminal, &payload);
    }

    /// Queues PTY output of `terminal` that ends with host record `seq` (0: unknown).
    pub fn output_at(&self, terminal: &str, bytes: &[u8], seq: u64) {
        if bytes.is_empty() {
            return;
        }
        let mut payload = Vec::with_capacity(1 + bytes.len());
        payload.push(KIND_OUTPUT);
        payload.extend_from_slice(bytes);
        let mut data = frame(&payload);
        if seq > 0 {
            let mut mark = vec![KIND_SEQ];
            mark.extend_from_slice(&seq.to_le_bytes());
            data.extend_from_slice(&frame(&mark));
        }
        self.send_frame(terminal, data);
    }

    /// Queues a checkpoint of `terminal` (#1194).
    pub fn checkpoint(&self, terminal: &str, checkpoint: &Checkpoint) {
        let mut payload = Vec::with_capacity(CHECKPOINT_HEAD + checkpoint.grid.len());
        payload.push(KIND_CHECKPOINT);
        payload.extend_from_slice(&checkpoint.seq.to_le_bytes());
        payload.extend_from_slice(&checkpoint.cols.to_le_bytes());
        payload.extend_from_slice(&checkpoint.rows.to_le_bytes());
        payload.extend_from_slice(&checkpoint.grid);
        if payload.len() as u64 > u64::from(MAX_PAYLOAD) {
            return;
        }
        self.send(terminal, &payload);
    }

    /// Queues the removal of the log of `terminal`: the next record starts a new one.
    pub fn reset(&self, terminal: &str) {
        if let Some(tx) = &self.tx {
            let _ = tx.try_send(Msg::Reset(terminal.to_string()));
        }
    }

    /// Queues a resize of `terminal`.
    pub fn resize(&self, terminal: &str, cols: u16, rows: u16) {
        let mut payload = vec![KIND_RESIZE];
        payload.extend_from_slice(&cols.to_le_bytes());
        payload.extend_from_slice(&rows.to_le_bytes());
        self.send(terminal, &payload);
    }

    /// Waits until the worker wrote everything queued so far (tests).
    pub fn flush(&self) {
        if let Some(tx) = &self.tx {
            let (done, wait) = sync_channel(1);
            if tx.send(Msg::Flush(done)).is_ok() {
                let _ = wait.recv();
            }
        }
    }

    fn send(&self, terminal: &str, payload: &[u8]) {
        self.send_frame(terminal, frame(payload));
    }

    fn send_frame(&self, terminal: &str, frame: Vec<u8>) {
        let Some(tx) = &self.tx else { return };
        let msg = Msg::Frame {
            id: terminal.to_string(),
            frame,
        };
        match tx.try_send(msg) {
            Ok(()) => self.overflowed.store(false, Ordering::Relaxed),
            Err(_) => {
                if !self.overflowed.swap(true, Ordering::Relaxed) {
                    crate::log::line(
                        &self.log,
                        "history",
                        "the history queue is full, records are dropped",
                    );
                }
            }
        }
    }
}

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(FRAME_HEAD + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&crc32(payload).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

/// Offsets `(start, end)` of the valid frames of `data` (a whole file) and the offset where
/// the valid part ends: a torn last frame or a damaged one ends it.
fn scan(data: &[u8]) -> (Vec<(usize, usize)>, usize) {
    if !data.starts_with(MAGIC) {
        return (Vec::new(), 0);
    }
    let mut frames = Vec::new();
    let mut at = MAGIC.len();
    while let Some(head) = data.get(at..at + FRAME_HEAD) {
        let len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let crc = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);
        let end = at + FRAME_HEAD + len as usize;
        let Some(payload) = data.get(at + FRAME_HEAD..end) else {
            break;
        };
        if len == 0 || len > MAX_PAYLOAD || crc32(payload) != crc {
            break;
        }
        frames.push((at, end));
        at = end;
    }
    (frames, at.max(MAGIC.len().min(data.len())))
}

fn decode(payload: &[u8]) -> Option<Record> {
    match payload.split_first()? {
        (&KIND_OUTPUT, body) => Some(Record::Output(body.to_vec())),
        (&KIND_RESIZE, [c0, c1, r0, r1]) => Some(Record::Resize {
            cols: u16::from_le_bytes([*c0, *c1]),
            rows: u16::from_le_bytes([*r0, *r1]),
        }),
        (&KIND_SEQ, body) => Some(Record::Seq(u64::from_le_bytes(body.try_into().ok()?))),
        (&KIND_CHECKPOINT, _) => Some(Record::Checkpoint(decode_checkpoint(payload))),
        _ => None,
    }
}

/// A checkpoint payload; a short one gives an empty checkpoint (size 0) that no rebuild uses,
/// so a damaged checkpoint does not hide the records after it.
fn decode_checkpoint(payload: &[u8]) -> Checkpoint {
    let Some(head) = payload.get(..CHECKPOINT_HEAD) else {
        return Checkpoint {
            seq: 0,
            cols: 0,
            rows: 0,
            grid: Vec::new(),
        };
    };
    let word = |at: usize| u16::from_le_bytes([head[at], head[at + 1]]);
    Checkpoint {
        seq: u64::from_le_bytes(head[1..9].try_into().expect("8 bytes")),
        cols: word(9),
        rows: word(11),
        grid: payload[CHECKPOINT_HEAD..].to_vec(),
    }
}

/// The records of the log at `path`, in order. A missing, foreign or empty file gives none;
/// a torn last record is ignored and a damaged one ends the list. Never fails.
pub fn read(path: &Path) -> Vec<Record> {
    let Ok(data) = std::fs::read(path) else {
        return Vec::new();
    };
    let (frames, _) = scan(&data);
    let mut records = Vec::with_capacity(frames.len());
    for (start, end) in frames {
        match decode(&data[start + FRAME_HEAD..end]) {
            Some(record) => records.push(record),
            None => break,
        }
    }
    records
}

struct Worker {
    dir: PathBuf,
    limits: Limits,
    log: PathBuf,
    open: HashMap<String, File>,
    sizes: HashMap<String, u64>,
    reported: HashSet<String>,
    total: u64,
    ready: bool,
    /// Size of a log at which a shrink could not drop anything (a checkpoint has to stay).
    stuck: HashMap<String, u64>,
}

impl Worker {
    fn new(dir: PathBuf, limits: Limits, log: PathBuf) -> Self {
        let mut sizes = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let Ok(meta) = entry.metadata() else { continue };
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    // The leftover of an interrupted shrink.
                    let _ = std::fs::remove_file(entry.path());
                } else if meta.is_file() {
                    sizes.insert(name, meta.len());
                }
            }
        }
        let total = sizes.values().sum();
        Self {
            dir,
            limits,
            log,
            open: HashMap::new(),
            sizes,
            reported: HashSet::new(),
            total,
            ready: false,
            stuck: HashMap::new(),
        }
    }

    /// Reports `error` for `id` once.
    fn report(&mut self, id: &str, error: &std::io::Error) {
        if self.reported.insert(id.to_string()) {
            crate::log::line(
                &self.log,
                "history",
                &format!("terminal {id}: could not write its history: {error}"),
            );
        }
    }

    fn append(&mut self, id: &str, frame: &[u8]) {
        if id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.') {
            return;
        }
        if let Err(e) = self.write(id, frame) {
            self.report(id, &e);
            return;
        }
        let size = self.sizes.get(id).copied().unwrap_or(0);
        let stuck = self
            .stuck
            .get(id)
            .map_or(0, |s| s + self.limits.per_terminal / 8);
        if size > self.limits.per_terminal && size >= stuck {
            self.shrink(id, self.limits.per_terminal - self.limits.per_terminal / 8);
        }
        self.enforce_total();
    }

    /// Removes the log of `id`.
    fn reset(&mut self, id: &str) {
        if id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.') {
            return;
        }
        self.open.remove(id);
        self.stuck.remove(id);
        let _ = std::fs::remove_file(self.dir.join(id));
        if let Some(old) = self.sizes.remove(id) {
            self.total = self.total.saturating_sub(old);
        }
    }

    fn write(&mut self, id: &str, frame: &[u8]) -> std::io::Result<()> {
        if !self.ready {
            prepare_dir(&self.dir)?;
            self.ready = true;
        }
        if !self.open.contains_key(id) {
            let file = open_private(&self.dir.join(id))?;
            let mut len = file.metadata()?.len();
            if len > 0 {
                // A torn or damaged tail of an earlier run would hide what is appended after it.
                let valid = scan(&std::fs::read(self.dir.join(id))?).1 as u64;
                if valid < len {
                    // An append handle cannot be cut on Windows: use a write handle.
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(self.dir.join(id))?
                        .set_len(valid)?;
                    len = valid;
                }
            }
            // A log found at start is counted already.
            if let Some(old) = self.sizes.insert(id.to_string(), len) {
                self.total = self.total.saturating_sub(old);
            }
            self.total += len;
            self.open.insert(id.to_string(), file);
        }
        let file = self.open.get_mut(id).expect("opened above");
        let size = self.sizes.get(id).copied().unwrap_or(0);
        let mut data = Vec::new();
        if size == 0 {
            data.extend_from_slice(MAGIC);
        }
        data.extend_from_slice(frame);
        if let Err(e) = file.write_all(&data) {
            // Cut a partial write, so the records after it stay readable.
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .open(self.dir.join(id))
                .and_then(|f| f.set_len(size));
            self.open.remove(id);
            return Err(e);
        }
        self.sizes.insert(id.to_string(), size + data.len() as u64);
        self.total += data.len() as u64;
        Ok(())
    }

    /// Drops the oldest records of `id` until it is at most `target` bytes.
    fn shrink(&mut self, id: &str, target: u64) {
        self.open.remove(id);
        let path = self.dir.join(id);
        let before = self.sizes.get(id).copied().unwrap_or(0);
        let result = std::fs::read(&path).and_then(|data| {
            let (frames, valid) = scan(&data);
            let mut keep = frames.len();
            let mut kept = (valid.max(MAGIC.len()) - MAGIC.len()) as u64;
            // Walk from the oldest: drop frames while the rest is over the target.
            let mut first = 0;
            while first < keep && MAGIC.len() as u64 + kept > target {
                let (start, end) = frames[first];
                kept -= (end - start) as u64;
                first += 1;
            }
            if frames.is_empty() {
                keep = 0;
            }
            // The log starts at a checkpoint, when it has one (#1194): the first one at or after
            // the cut, else the last one (the file then stays above the target).
            let is_checkpoint = |i: usize| data[frames[i].0 + FRAME_HEAD] == KIND_CHECKPOINT;
            if (0..keep).any(is_checkpoint) {
                first = (first..keep)
                    .find(|&i| is_checkpoint(i))
                    .unwrap_or_else(|| (0..keep).rev().find(|&i| is_checkpoint(i)).unwrap_or(0));
            }
            let mut out = MAGIC.to_vec();
            if keep > first {
                out.extend_from_slice(&data[frames[first].0..frames[keep - 1].1]);
            }
            let tmp = self.dir.join(format!(".{id}.tmp"));
            let mut file = open_private_truncated(&tmp)?;
            file.write_all(&out)?;
            drop(file);
            std::fs::rename(&tmp, &path)?;
            Ok(out.len() as u64)
        });
        match result {
            Ok(size) => {
                if size >= before {
                    self.stuck.insert(id.to_string(), before);
                } else {
                    self.stuck.remove(id);
                }
                self.sizes.insert(id.to_string(), size);
                self.total = self.total.saturating_sub(before) + size;
            }
            Err(e) => self.report(id, &e),
        }
    }

    /// While all logs together are over the cap, the largest one loses its oldest data.
    fn enforce_total(&mut self) {
        while self.total > self.limits.total {
            let Some((id, size)) = self
                .sizes
                .iter()
                .max_by_key(|(_, s)| **s)
                .map(|(id, s)| (id.clone(), *s))
            else {
                return;
            };
            let excess = self.total - self.limits.total;
            let target = size.saturating_sub(excess + size / 8);
            self.shrink(&id, target);
            if self.sizes.get(&id).copied().unwrap_or(0) >= size {
                return;
            }
        }
    }
}

fn prepare_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    owner_only_acl(dir);
    Ok(())
}

/// Windows: the directory keeps only the user's own access (no inherited entries). Best
/// effort: `%LOCALAPPDATA%` is the user's profile already.
#[cfg(windows)]
#[allow(clippy::disallowed_methods)] // CREATE_NO_WINDOW is set below.
fn owner_only_acl(dir: &Path) {
    use std::os::windows::process::CommandExt;
    if let Ok(user) = std::env::var("USERNAME") {
        let _ = std::process::Command::new("icacls")
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .arg(dir)
            .args(["/inheritance:r", "/grant:r"])
            .arg(format!("{user}:(OI)(CI)F"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

fn open_private(path: &Path) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

fn open_private_truncated(path: &Path) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
