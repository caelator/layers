//! Shared test utilities for layers integration tests.
//!
//! WAIT_TIMEOUT: on Unix, we use a self-pipe trick to allow SIGCHLD to wake
//! the select() call so that waitpid() doesn't block indefinitely.

use std::ffi::OsStr;
use std::io::{self, Read};
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Global flag set by the SIGUSR1 signal handler.
static SIGNALED: AtomicBool = AtomicBool::new(false);

extern "C" fn signal_handler(_sig: i32) {
    SIGNALED.store(true, Ordering::SeqCst);
}

/// Register a signal handler for the given signal.
/// The handler sets the global `SIGNALED` flag.
/// Only one handler can be active at a time (safe in single-threaded tests).
pub fn set_signal_handler(sig: libc::c_int) -> anyhow::Result<()> {
    unsafe {
        let mut act = std::mem::zeroed::<libc::struct_sigaction>();
        act.sa_sigaction = signal_handler as usize;
        act.sa_flags = libc::SA_SIGINFO;
        if libc::sigaction(sig, &act, std::ptr::null_mut()) != 0 {
            anyhow::bail!("sigaction failed: {}", io::Error::last_os_error());
        }
    }
    Ok(())
}

/// A self-pipe used to wake select() when SIGCHLD arrives.
struct SigchldPipe {
    read_fd: i32,
    write_fd: i32,
}

impl SigchldPipe {
    fn new() -> io::Result<Self> {
        let mut fds = [0i32, 0i32];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { read_fd: fds[0], write_fd: fds[1] })
    }

    fn try_read(&self) -> bool {
        let mut buf = [0u8; 1];
        unsafe { libc::read(self.read_fd, buf.as_mut_ptr() as *mut libc::c_void, 1) > 0 }
    }
}

impl AsRawFd for SigchldPipe {
    fn as_raw_fd(&self) -> i32 {
        self.read_fd
    }
}

impl Drop for SigchldPipe {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.read_fd);
            libc::close(self.write_fd);
        }
    }
}

/// Wait for a child process to exit within `timeout`.
///
/// Uses a self-pipe + SIGCHLD handler so that `select()` wakes immediately
/// when the child exits, rather than blocking for the full timeout.
pub fn wait_timeout(child: &mut Child, timeout: Duration) -> anyhow::Result<WaitStatus> {
    let mut pipe = SigchldPipe::new()?;

    // Set SIGCHLD to non-deterministic signals (we don't use SA_RESTART,
    // so it will interrupt select())
    unsafe {
        let mut act = std::mem::zeroed::<libc::struct_sigaction>();
        act.sa_sigaction = libc::SIG_DFL;
        act.sa_flags = 0;
        libc::sigaction(libc::SIGCHLD, &act, std::ptr::null_mut());
    }

    let deadline = Instant::now() + timeout;

    loop {
        // Use select() on the pipe with the remaining timeout
        let remaining = deadline.saturating_duration_since(Instant::now());
        let ts = libc::timespec {
            tv_sec: remaining.as_secs() as libc::time_t,
            tv_nsec: remaining.subsec_nanos() as libc::c_long,
        };
        let mut fdset = unsafe { std::mem::zeroed::<libc::fd_set>() };

        // FD_SET on the pipe read fd
        unsafe { libc::FD_SET(self.read_fd, &mut fdset) };

        let ret = unsafe {
            libc::select(
                self.read_fd + 1,
                &mut fdset,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &ts,
            )
        };

        // Try to reap the child
        match child.try_wait()? {
            Some(status) => return Ok(WaitStatus::Exited(status)),
            None => {
                // Still running. Check if we hit the deadline.
                if Instant::now() >= deadline {
                    // Kill child and wait for it to exit
                    let _ = child.kill();
                    let status = child.wait()?;
                    return Ok(WaitStatus::TimedOut(status));
                }
                // select returned 0 (timeout) or EINTR — retry
                if ret == 0 || ret == -1 {
                    continue;
                }
                // SIGCHLD fired — loop and try waitpid again
                let _ = pipe.try_read(); // consume wake byte
            }
        }
    }
}

#[derive(Debug)]
pub enum WaitStatus {
    /// Child exited within the timeout.
    Exited(std::process::ExitStatus),
    /// Timeout elapsed before child exited. Child has been killed.
    TimedOut(std::process::ExitStatus),
}

#[test]
fn set_signal_handler() {
    // Register handler for SIGUSR1
    set_signal_handler(libc::SIGUSR1).expect("signal handler registration failed");

    // Flag starts false
    assert!(!SIGNALED.load(Ordering::SeqCst));

    // Send ourselves SIGUSR1
    let ret = unsafe { libc::kill(libc::getpid(), libc::SIGUSR1) };
    assert_eq!(ret, 0, "kill syscall failed");

    // Handler sets the flag
    assert!(SIGNALED.load(Ordering::SeqCst), "SIGNALED flag should be set after SIGUSR1");

    // Reset
    SIGNALED.store(false, Ordering::SeqCst);
    assert!(!SIGNALED.load(Ordering::SeqCst));
}
