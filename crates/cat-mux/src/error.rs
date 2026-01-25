//! Error types for the multiplexer

use thiserror::Error;

use crate::state::RadioHandle;

/// Errors that can occur in the multiplexer
#[derive(Debug, Error)]
pub enum MuxError {
    /// Radio not found
    #[error("radio not found: {0}")]
    RadioNotFound(String),

    /// Radio already exists
    #[error("radio already exists: {0}")]
    RadioExists(String),

    /// No active radio
    #[error("no active radio selected")]
    NoActiveRadio,

    /// Amplifier not configured
    #[error("amplifier output not configured")]
    NoAmplifier,

    /// Translation error
    #[error("translation error: {0}")]
    TranslationError(String),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Protocol error
    #[error("protocol error: {0}")]
    ProtocolError(#[from] cat_protocol::ProtocolError),

    /// Switching blocked (lockout active)
    #[error("switching blocked: lockout expires in {remaining_ms}ms")]
    SwitchingLocked {
        /// Radio that requested to become active
        requested: RadioHandle,
        /// Currently active radio
        current: RadioHandle,
        /// Time remaining in lockout (milliseconds)
        remaining_ms: u64,
    },
}
