//! Synthetic service lifecycle probe for platform CI; never starts without explicit arguments.
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

fn worker(config: &Path, stopping: &AtomicBool) -> Result<(), String> {
    while !stopping.load(Ordering::SeqCst) {
        std::fs::write(config.with_extension("heartbeat"), b"running").map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(50));
    }
    std::fs::write(config.with_extension("stopped"), b"stopped gracefully").map_err(|e| e.to_string())
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut action = "status".to_owned();
    let mut config = None;
    let mut account = String::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--service" => action = "run".into(),
            "--config" => config = args.next().map(std::path::PathBuf::from),
            "--service-action" => action = args.next().ok_or("missing action")?,
            "--account" => account = args.next().ok_or("missing account")?,
            _ => return Err("unexpected probe argument".into()),
        }
    }
    let config = config.ok_or("--config is required")?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    use pdfcraft_hot_folder_service as service;
    match action.as_str() {
        "run" => return service::run(&config, worker),
        "install" => service::install(&executable, &config, &account)?,
        "uninstall" => service::uninstall(&executable, &config)?,
        "start" => service::start(&executable, &config)?,
        "stop" => service::stop(&executable, &config)?,
        "enable" => service::set_enabled(true, &executable, &config)?,
        "disable" => service::set_enabled(false, &executable, &config)?,
        "status" => {}
        _ => return Err("unknown action".into()),
    }
    let state = service::status(&executable, &config)?;
    println!("installed={} enabled={} running={} account={}", state.installed, state.enabled, state.running, state.account);
    Ok(())
}
