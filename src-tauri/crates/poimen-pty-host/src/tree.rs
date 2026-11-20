//! Ends the process tree of a terminal (#968).
//!
//! Unix: every process whose session id is the child's pid (the child is a session leader, its
//! jobs run in groups of their own, so `killpg` would miss them). SIGHUP first, SIGKILL after the
//! grace time. A process that left the session (`setsid`, a daemon) is not touched, as in a real
//! terminal. Windows: a job object the child is assigned to at spawn; ending it terminates the
//! job. The host never signals pid 1, itself, or a process outside the session (job).
#![allow(unsafe_code)]

use std::time::{Duration, Instant};

pub use poimen_protocol::host::TreeEnd;

/// How often the sweep looks again while it waits for processes to go.
const POLL: Duration = Duration::from_millis(50);

#[cfg(unix)]
pub use self::unix::Tree;
#[cfg(windows)]
pub use self::windows::Tree;

fn wait_until(deadline: Instant, mut done: impl FnMut() -> bool) -> bool {
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(unix)]
mod unix {
    use super::*;

    /// The session of a terminal: its id is the pid of the child (the session leader).
    pub struct Tree {
        sid: Option<u32>,
    }

    impl Tree {
        pub fn new(child_pid: Option<u32>) -> Tree {
            // The host must not sweep its own session, nor pid 0 or 1.
            let sid = child_pid.filter(|&p| p > 1 && p != std::process::id());
            let sid = sid.filter(|&p| {
                // SAFETY: getsid has no memory effects.
                let own = unsafe { libc::getsid(0) };
                own < 0 || p != own as u32
            });
            Tree { sid }
        }

        /// SIGHUP to the session, SIGKILL to what is left after `grace`.
        pub fn end(&self, grace: Duration) -> TreeEnd {
            let Some(sid) = self.sid else {
                return TreeEnd::Unconfirmed;
            };
            if members(sid).is_empty() {
                return TreeEnd::Ended;
            }
            signal_all(sid, libc::SIGHUP);
            let started = Instant::now();
            if wait_until(started + grace, || members(sid).is_empty()) {
                return TreeEnd::Ended;
            }
            signal_all(sid, libc::SIGKILL);
            let confirm = Instant::now() + Duration::from_millis(500);
            if wait_until(confirm, || members(sid).is_empty()) {
                TreeEnd::Ended
            } else {
                TreeEnd::Unconfirmed
            }
        }
    }

    fn signal_all(sid: u32, signal: libc::c_int) {
        for pid in members(sid) {
            // Looked up again right before the signal: a pid that left the session meanwhile
            // (or was reused) is not signalled.
            if session_of(pid) == Some(sid) {
                // SAFETY: kill sends a signal, no memory is touched.
                unsafe { libc::kill(pid as libc::pid_t, signal) };
            }
        }
    }

    fn session_of(pid: u32) -> Option<u32> {
        // SAFETY: getsid has no memory effects.
        let sid = unsafe { libc::getsid(pid as libc::pid_t) };
        u32::try_from(sid).ok()
    }

    /// The live processes of session `sid` (zombies are gone for our purpose), never pid 1 or
    /// this process.
    pub(super) fn members(sid: u32) -> Vec<u32> {
        let me = std::process::id();
        all_pids()
            .into_iter()
            .filter(|&p| p > 1 && p != me && session_of(p) == Some(sid) && !is_zombie(p))
            .collect()
    }

    #[cfg(target_os = "linux")]
    fn all_pids() -> Vec<u32> {
        let Ok(dir) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
            .collect()
    }

    #[cfg(target_os = "linux")]
    fn is_zombie(pid: u32) -> bool {
        // `pid (comm) S ...`: comm may hold spaces and parentheses, so look after the last ")".
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|s| {
                s.rsplit_once(')')
                    .map(|(_, rest)| rest.trim_start().starts_with('Z'))
            })
            .unwrap_or(false)
    }

    #[cfg(target_os = "macos")]
    fn all_pids() -> Vec<u32> {
        // SAFETY: a null buffer asks for the size; the second call fills a buffer of that size.
        unsafe {
            let bytes = libc::proc_listallpids(std::ptr::null_mut(), 0);
            if bytes <= 0 {
                return Vec::new();
            }
            // Room for processes started between the two calls.
            let mut pids =
                vec![0 as libc::pid_t; bytes as usize / std::mem::size_of::<libc::pid_t>() + 64];
            let size = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
            let got = libc::proc_listallpids(pids.as_mut_ptr().cast(), size);
            if got <= 0 {
                return Vec::new();
            }
            pids.truncate(got as usize / std::mem::size_of::<libc::pid_t>());
            pids.into_iter()
                .filter_map(|p| u32::try_from(p).ok())
                .collect()
        }
    }

    #[cfg(target_os = "macos")]
    fn is_zombie(pid: u32) -> bool {
        // SAFETY: kinfo is plain data the call fills.
        unsafe {
            let mut info: libc::proc_bsdinfo = std::mem::zeroed();
            let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
            let got = libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            );
            got == size && info.pbi_status == libc::SZOMB
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn all_pids() -> Vec<u32> {
        Vec::new()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn is_zombie(_pid: u32) -> bool {
        false
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::os::windows::io::RawHandle;
    use std::ptr;
    use winapi::shared::minwindef::FALSE;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::jobapi2::{
        AssignProcessToJobObject, CreateJobObjectW, QueryInformationJobObject,
        SetInformationJobObject, TerminateJobObject,
    };
    use winapi::um::winnt::{
        HANDLE, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
        JobObjectExtendedLimitInformation,
    };

    /// A job object holding the child and everything it starts; closing it kills what is left.
    pub struct Tree {
        job: Option<HANDLE>,
    }

    // SAFETY: a job handle is usable from any thread.
    unsafe impl Send for Tree {}
    unsafe impl Sync for Tree {}

    impl Tree {
        /// Creates the job and assigns the child (`process`) to it; without a job the tree is
        /// reported unconfirmed.
        pub fn new(process: Option<RawHandle>) -> Tree {
            let job = process.and_then(|p| unsafe { create_job(p as HANDLE) });
            Tree { job }
        }

        pub fn end(&self, grace: Duration) -> TreeEnd {
            let Some(job) = self.job else {
                return TreeEnd::Unconfirmed;
            };
            // SAFETY: the handle is ours and open until drop.
            unsafe { TerminateJobObject(job, 1) };
            let deadline = Instant::now() + grace.min(Duration::from_secs(2));
            if wait_until(deadline, || active(job) == Some(0)) {
                TreeEnd::Ended
            } else {
                TreeEnd::Unconfirmed
            }
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            if let Some(job) = self.job {
                // SAFETY: closes our own handle once.
                unsafe { CloseHandle(job) };
            }
        }
    }

    unsafe fn create_job(process: HANDLE) -> Option<HANDLE> {
        unsafe {
            let job = CreateJobObjectW(ptr::null_mut(), ptr::null());
            if job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&mut info as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set == FALSE || AssignProcessToJobObject(job, process) == FALSE {
                CloseHandle(job);
                return None;
            }
            Some(job)
        }
    }

    /// Processes still in the job.
    fn active(job: HANDLE) -> Option<u32> {
        // SAFETY: the info struct is plain data the call fills.
        unsafe {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
            let ok = QueryInformationJobObject(
                job,
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                ptr::null_mut(),
            );
            (ok != FALSE).then_some(info.ActiveProcesses)
        }
    }
}
