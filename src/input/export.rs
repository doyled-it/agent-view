use std::io;

use crate::core::tmux::TmuxError;

#[derive(thiserror::Error, Debug)]
pub enum ExportError {
    #[error("cannot find home directory")]
    NoHomeDir,
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("tmux error: {0}")]
    Tmux(#[from] TmuxError),
}

pub type ExportResult<T> = Result<T, ExportError>;

pub fn export_session_log(
    tmux_session: &str,
    title: &str,
    session_id: &str,
) -> ExportResult<String> {
    let home = dirs::home_dir().ok_or(ExportError::NoHomeDir)?;
    let export_dir = home.join(".agent-view").join("exports");
    std::fs::create_dir_all(&export_dir)?;

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
        std::fs::copy(&log_path, &filepath)?;
        return Ok(filepath.to_string_lossy().to_string());
    }

    // Fallback to live capture
    let output = crate::core::tmux::capture_pane(tmux_session, Some(-10000), false)?;
    std::fs::write(&filepath, &output)?;

    Ok(filepath.to_string_lossy().to_string())
}
