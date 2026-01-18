//! Error types for CAT detection

use thiserror::Error;

/// Errors that can occur during detection
#[derive(Debug, Error)]
pub enum DetectError {
    /// Failed to enumerate serial ports
    #[error("failed to enumerate ports: {0}")]
    EnumerationFailed(String),

    /// Failed to open serial port
    #[error("failed to open port {port}: {reason}")]
    OpenFailed { port: String, reason: String },

    /// Timeout waiting for response
    #[error("timeout probing {port} with {protocol}")]
    Timeout { port: String, protocol: String },

    /// I/O error during probe
    #[error("I/O error on {port}: {reason}")]
    IoError { port: String, reason: String },

    /// Port busy or in use
    #[error("port {0} is busy or in use")]
    PortBusy(String),

    /// Serial port error
    #[error("serial port error: {0}")]
    SerialPort(#[from] serialport::Error),
}
