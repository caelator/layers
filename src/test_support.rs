#![allow(clippy::fn_to_numeric_cast)]
//! Test utilities for layers integration tests.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

// ─── Workspace isolation ─────────────────────────────────────────────────────

pub fn workspace_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub fn workspace_guard() -> MutexGuard<'static, ()> {
    workspace_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub struct TestWorkspace {
    _guard: MutexGuard<'static, ()>,
    original_root: Option<OsString>,
    root: PathBuf,
}

impl TestWorkspace {
    #[allow(unsafe_code)]
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
    #[allow(unsafe_code)]
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
#[allow(unsafe_code)]
pub fn set_usr1_handler() -> io::Result<()> {
    unsafe {
        let mut act: libc::sigaction = std::mem::zeroed();
        // sighandler_t is usize on macOS BSD.
        // transmute with explicit type annotation to get the function address as usize.
        let fn_ptr: usize = usr1_handler as *const () as usize;
        act.sa_sigaction = fn_ptr as libc::sighandler_t;
        act.sa_flags = libc::SA_SIGINFO;
        if libc::sigemptyset(&raw mut act.sa_mask) != 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::sigaction(libc::SIGUSR1, &raw const act, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

// ─── Child process timeout ────────────────────────────────────────────────────

#[derive(Debug)]
#[allow(dead_code)]
pub enum ChildResult {
    Exited(std::process::ExitStatus),
    TimedOut,
    Err(io::Error),
}

/// Wait for a child process to exit, with a hard timeout.
/// Uses polling so it's portable and deadlock-free.
/// On timeout the child is killed with SIGKILL.
#[allow(dead_code)]
pub fn wait_child_timeout(child: &mut Child, timeout: Duration) -> ChildResult {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return ChildResult::Exited(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return ChildResult::TimedOut;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return ChildResult::Err(e),
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[test]
#[allow(unsafe_code)]
fn usr1_handler_sets_flag() {
    assert!(!SIGNALED.load(Ordering::SeqCst));

    set_usr1_handler().expect("failed to register SIGUSR1 handler");

    let pid = unsafe { libc::getpid() };
    let ret = unsafe { libc::kill(pid, libc::SIGUSR1) };
    assert_eq!(ret, 0, "kill failed: {}", io::Error::last_os_error());

    // Give the kernel a moment to deliver the signal
    std::thread::sleep(Duration::from_millis(50));

    assert!(SIGNALED.load(Ordering::SeqCst), "SIGNALED flag should be set after SIGUSR1");

    SIGNALED.store(false, Ordering::SeqCst);
    assert!(!SIGNALED.load(Ordering::SeqCst));
}
