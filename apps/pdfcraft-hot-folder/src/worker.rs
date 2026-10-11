//! Background processing loop used by the manual worker and the system-service host.
use pdfcraft_engine::hot_folder as hf;
use std::path::Path;

pub fn check_identity() -> Result<(), String> {
    #[cfg(unix)]
    if rustix::process::geteuid().is_root() {
        return Err("Run folder processing as the configured non-root service account; administrator mode is only for setup".into());
    }
    Ok(())
}

pub fn run(config: &Path, should_stop: impl Fn() -> bool) -> Result<(), String> {
    check_identity()?;
    if !config.exists() {
        hf::initialize_config(config)?;
    }
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(config.with_extension("worker.lock"))
        .map_err(|e| e.to_string())?;
    if lock.try_lock().is_err() {
        return Ok(());
    }
    while !should_stop() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hf::poll_cancelable(config, None, &should_stop)))
            .unwrap_or_else(|_| Err("processing failed safely; source files were retained".into()));
        match result {
            Ok(_) => {
                hf::record_worker_error(config, None)?;
            }
            Err(error) => {
                hf::record_worker_error(config, Some(&error))?;
            }
        }
        if !should_stop() {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn hot_folder_worker_initializes_missing_stopped_configuration_as_its_account() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/tasks.json");
        #[cfg(unix)]
        if rustix::process::geteuid().is_root() {
            assert!(super::run(&path, || true).unwrap_err().contains("non-root"));
            assert!(!path.exists());
            return;
        }
        super::run(&path, || true).unwrap();
        assert!(pdfcraft_engine::hot_folder::load_config(&path).unwrap().tasks.is_empty());
    }
}
