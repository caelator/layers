//! Test utilities for layers integration tests.
//!
//! ## Signal handling
//! `set_usr1_handler` registers a `SIGUSR1` handler that sets a global flag.
//! The handler is async-signal-safe (only sets an `AtomicBool`).
//!
//! ## Child process timeouts
//! `wait_child_timeout` waits for a child with a hard deadline.  It uses a
//! companion thread so the calling test thread is never blocked indefinitely.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

// ─── Workspace isolation ─────────────────────────────────────────────────────

pub fn workspace_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub fn workspace_guard() -> MutexGuard<'static, ()> {
    workspace_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct TestWorkspace {
    _guard: MutexGuard<'static ()>,
    original_root: Option<OsString>,
    root: PathBuf,
}

impl TestWorkspace {
    pub fn new(name: &str) -> Self {
        let guard = workspace_guard();
        let root = std::env::temp_dir().join(format!(
            "layers-tests-{}-{}",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("memoryport")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();

        let original_root = std::env::var_os("LAYERS_WORKSPACE_ROOT");
        unsafe {
            std::env::set_var("LAYERS_WORKSPACE_ROOT", &root);
        }

        Self {
            _guard: guard,
            original_root,
            root,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        if let Some(value) = &self.original_root {
            unsafe {
                std::env::set_var("LAYERS_WORKSPACE_ROOT", value);
            }
        } else {
            unsafe {
                std::env::remove_var("LAYERS_WORKSPACE_ROOT");
            }
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

// ─── Signal handling ─────────────────────────────────────────────────────────

static SIGNALED: AtomicBool = AtomicBool::new(false);

extern "C" fn usr1_handler(_sig: libc::c_int) {
    SIGNALED.store(true, Ordering::SeqCst);
}

/// Register `SIGUSR1` handler that sets the global `SIGNALED` flag.
pub fn set_usr1_handler() -> io::Result<()> {
    unsafe {
        let mut act = std::mem::zeroed::<libc::sigaction>();
        act.sa_sigaction = usr1_handler as libc::sighandler_t;
        act.sa_flags = libc::SA_SIGINFO;
        if libc::sigaction(libc::SIGUSR1, &act, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

// ─── Child process timeout ────────────────────────────────────────────────────

/// Result of `wait_child_timeout`.
pub enum ChildResult {
    /// Child exited cleanly within the timeout.
    Exited(std::process::ExitStatus),
    /// Timeout elapsed; child has been killed with SIGKILL.
    TimedOut,
    /// An I/O error occurred.
    Err(io::Error),
}

/// Wait for a child process to exit, with a hard timeout.
///
/// Uses a companion thread so the calling test thread is never blocked
/// indefinitely.  On timeout the child is killed with SIGKILL.
pub fn wait_child_timeout(child: &mut Child, timeout: Duration) -> ChildResult {
    let result = Arc::new(Mutex::new(None));
    let result2 = Arc::clone(&result);

    let handle = std::thread::spawn(move || {
        // Wait on the child indefinitely (this is the background thread)
        let status = child.wait();
        let mut guard = result2.lock().unwrap();
        *guard = Some(match status {
            Ok(s) => ChildResult::Exited(s),
            Err(e) => ChildResult::Err(e),
        });
    });

    // Wait for either the child to exit or the timeout to fire
    let start = Instant::now();
    loop {
        // Check immediately without sleeping first
        let guard = result.lock().unwrap();
        if let Some(ref r) = *guard {
            // Child already exited — join the thread and return
            drop(guard);
            let _ = handle.join();
            return r.clone();
        }
        drop(guard);

        if start.elapsed() >= timeout {
            // Timeout — kill the child and wait for the thread
            let _ = child.kill();
            let _ = child.wait();
            let _ = handle.join();
            return ChildResult::TimedOut;
        }

        std::thread::sleep(Duration::from_millis(20));
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn usr1_handler_sets_flag() {
    // Flag starts false
    assert!(!SIGNALED.load(Ordering::SeqCst));

    // Register handler
    set_usr1_handler().expect("failed to register SIGUSR1 handler");

    // Send ourselves SIGUSR1
    let pid = unsafe { libc::getpid() };
    let ret = unsafe { libc::kill(pid, libc::SIGUSR1) };
    assert_eq!(ret, 0, "kill failed: {}", io::Error::last_os_error());

    // Handler must have set the flag
    assert!(SIGNALED.load(Ordering::SeqCst), "SIGNALED flag should be set after SIGUSR1");

    // Reset for next test
    SIGNALED.store(false, Ordering::SeqCst);
    assert!(!SIGNALED.load(Ordering::SeqCst));
}
