//! Service definitions contain arguments as data, never shell commands.

#[cfg(any(target_os = "linux", target_os = "macos", test))]
pub fn account(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || value == "root"
        || value == "0"
        || value.starts_with('-')
        || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err("Choose an existing, non-root service account".into());
    }
    Ok(())
}

#[cfg(any(windows, test))]
pub fn windows_command(executable: &str, config: &str) -> Result<String, String> {
    if [executable, config].iter().any(|s| s.contains('"') || s.chars().any(char::is_control) || s.ends_with('\\')) {
        return Err("Service paths cannot contain quotes, control characters or a trailing backslash".into());
    }
    Ok(format!("\"{executable}\" --service --config \"{config}\""))
}

#[cfg(any(windows, test))]
pub fn windows_command_matches(command: &str, executable: &str, config: &str) -> Result<bool, String> {
    let quoted = windows_command(executable, config)?;
    // MSI quotes both paths; windows-service quotes only arguments containing spaces.
    // Restrict matching to these two known spellings rather than parse arbitrary shell text.
    let argument = |value: &str| if value.contains(' ') { format!("\"{value}\"") } else { value.into() };
    Ok(command == quoted || command == format!("{} --service --config {}", argument(executable), argument(config)))
}

#[cfg(any(target_os = "linux", test))]
fn unit_argument(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%").replace('$', "$$"))
}

#[cfg(any(target_os = "linux", test))]
pub fn systemd(executable: &str, config: &str, user: &str) -> Result<String, String> {
    account(user)?;
    Ok(format!(
        "# PdfCraft Hot Folder system service\n[Unit]\nDescription=PdfCraft Hot Folder\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nUser={user}\nExecStart={} --service --config {}\nRestart=on-failure\nRestartSec=10\nTimeoutStopSec=120\nUMask=0077\nNoNewPrivileges=true\nPrivateTmp=true\nProtectSystem=full\n\n[Install]\nWantedBy=multi-user.target\n",
        unit_argument(executable),
        unit_argument(config)
    ))
}

#[cfg(any(target_os = "macos", test))]
pub fn launchd_enabled(output: &str) -> Result<bool, String> {
    let label = format!("\"{}\"", crate::UNIX_LABEL);
    for line in output.lines() {
        if let Some((key, value)) = line.trim().split_once("=>")
            && key.trim() == label
        {
            return match value.trim() {
                "true" | "disabled" => Ok(false),
                "false" | "enabled" => Ok(true),
                _ => Err("Unrecognized launchd startup state".into()),
            };
        }
    }
    Ok(true)
}

#[cfg(any(target_os = "macos", test))]
fn xml(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(any(target_os = "macos", test))]
pub fn launchd(executable: &str, config: &str, user: &str) -> Result<String, String> {
    account(user)?;
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- PdfCraft Hot Folder system service -->\n<plist version=\"1.0\"><dict>\n<key>Label</key><string>{}</string>\n<key>UserName</key><string>{}</string>\n<key>ProgramArguments</key><array><string>{}</string><string>--service</string><string>--config</string><string>{}</string></array>\n<key>RunAtLoad</key><true/>\n<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\n<key>ThrottleInterval</key><integer>10</integer>\n<key>ExitTimeOut</key><integer>120</integer>\n<key>Umask</key><integer>63</integer>\n<key>ProcessType</key><string>Background</string>\n</dict></plist>\n",
        crate::UNIX_LABEL,
        xml(user),
        xml(executable),
        xml(config)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launchd_reads_both_boolean_and_modern_disabled_output() {
        for value in ["true", "disabled"] {
            assert!(!launchd_enabled(&format!("disabled services = {{\n\t\"{}\" => {value}\n}}", crate::UNIX_LABEL)).unwrap());
        }
        for value in ["false", "enabled"] {
            assert!(launchd_enabled(&format!("\"{}\" => {value}", crate::UNIX_LABEL)).unwrap());
        }
        assert!(launchd_enabled("\"another.service\" => disabled").unwrap());
        assert!(launchd_enabled(&format!("\"{}\" => unknown", crate::UNIX_LABEL)).is_err());
    }
    #[test]
    fn generated_units_cannot_inject_arguments_or_run_as_root() {
        let text = systemd("/opt/Pdf Craft/worker", "/home/test/A%$B/tasks.json", "test").unwrap();
        assert!(text.contains("User=test\n"));
        assert!(text.contains("\"/home/test/A%%$$B/tasks.json\""));
        assert!(text.contains("WantedBy=multi-user.target"));
        assert!(systemd("/bin/worker", "/tasks.json", "root").is_err());
        assert!(systemd("/bin/worker", "/tasks.json", "user\nExecStart=bad").is_err());
        let plist = launchd("/Applications/PdfCraft Hot Folder.app/Contents/MacOS/worker", "/Users/A & B/<tasks>.json", "test").unwrap();
        assert!(plist.contains("A &amp; B/&lt;tasks&gt;.json"));
        assert!(plist.contains("<key>UserName</key><string>test</string>"));
        assert!(!plist.contains("/bin/sh"));
    }
    #[test]
    fn windows_binary_and_config_are_separate_quoted_arguments() {
        assert_eq!(
            windows_command(r"C:\Program Files\PdfCraft\worker.exe", r"C:\ProgramData\PdfCraft\tasks.json").unwrap(),
            r#""C:\Program Files\PdfCraft\worker.exe" --service --config "C:\ProgramData\PdfCraft\tasks.json""#
        );
        assert!(windows_command("bad\"exe", "file").is_err());
        assert!(
            windows_command_matches(
                r#""C:\Program Files\worker.exe" --service --config C:\tasks.json"#,
                r"C:\Program Files\worker.exe",
                r"C:\tasks.json"
            )
            .unwrap()
        );
        assert!(
            !windows_command_matches(
                r"C:\Program Files\worker.exe --service --config C:\tasks.json",
                r"C:\Program Files\worker.exe",
                r"C:\tasks.json"
            )
            .unwrap()
        );
    }
}
