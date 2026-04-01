use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

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
    _guard: MutexGuard<'static, ()>,
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
                .unwrap()
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
