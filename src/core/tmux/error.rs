use std::io;

#[derive(thiserror::Error, Debug)]
pub enum TmuxError {
    #[error("tmux command failed: {0}")]
    CommandFailed(String),
    #[error("tmux capture failed")]
    CaptureFailed,
    #[error("tmux attach failed: {0}")]
    AttachFailed(String),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

pub type TmuxResult<T> = Result<T, TmuxError>;
