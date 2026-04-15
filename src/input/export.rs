pub fn export_session_log(
    tmux_session: &str,
    title: &str,
    session_id: &str,
) -> Result<String, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let export_dir = home.join(".agent-view").join("exports");
    std::fs::create_dir_all(&export_dir)
        .map_err(|e| format!("Cannot create exports dir: {}", e))?;

    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let safe_name: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .take(30)
        .collect();
    let filename = format!("{}-{}.log", safe_name, timestamp);
    let filepath = export_dir.join(&filename);

    // Try continuous log file first
    let log_path = crate::core::logger::session_log_path(session_id);
    if log_path.exists() {
        std::fs::copy(&log_path, &filepath).map_err(|e| format!("Copy failed: {}", e))?;
        return Ok(filepath.to_string_lossy().to_string());
    }

    // Fallback to live capture
    let output = crate::core::tmux::capture_pane(tmux_session, Some(-10000), false)
        .map_err(|e| format!("Capture failed: {}", e))?;
    std::fs::write(&filepath, &output).map_err(|e| format!("Write failed: {}", e))?;

    Ok(filepath.to_string_lossy().to_string())
}
