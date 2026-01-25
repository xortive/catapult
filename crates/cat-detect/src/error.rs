//! Error types for CAT detection

use thiserror::Error;

/// Errors that can occur during detection
#[derive(Debug, Error)]
pub enum DetectError {
    /// Failed to enumerate serial ports
    #[error("failed to enumerate ports: {0}")]
    EnumerationFailed(String),

    /// Serial port error
    #[error("serial port error: {0}")]
    SerialPort(#[from] serialport::Error),
}
