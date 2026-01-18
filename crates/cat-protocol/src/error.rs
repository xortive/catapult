//! Error types for CAT protocol parsing and encoding

use thiserror::Error;

/// Errors that can occur while parsing protocol data
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Buffer is incomplete - need more data
    #[error("incomplete data: need {needed} more bytes")]
    Incomplete { needed: usize },

    /// Invalid frame structure
    #[error("invalid frame: {0}")]
    InvalidFrame(String),

    /// Unknown or unsupported command
    #[error("unknown command: {0}")]
    UnknownCommand(String),

    /// Invalid BCD encoding
    #[error("invalid BCD digit: {0}")]
    InvalidBcd(u8),

    /// Invalid frequency value
    #[error("invalid frequency: {0}")]
    InvalidFrequency(String),

    /// Invalid mode value
    #[error("invalid mode: {0}")]
    InvalidMode(String),

    /// Invalid address (for CI-V)
    #[error("invalid address: 0x{0:02X}")]
    InvalidAddress(u8),

    /// Checksum mismatch
    #[error("checksum mismatch: expected 0x{expected:02X}, got 0x{actual:02X}")]
    ChecksumMismatch { expected: u8, actual: u8 },
}

/// Higher-level protocol errors
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// Parse error
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),

    /// Command cannot be translated to target protocol
    #[error("cannot translate command: {0}")]
    UntranslatableCommand(String),

    /// Feature not supported by target radio
    #[error("feature not supported: {0}")]
    UnsupportedFeature(String),

    /// Communication timeout
    #[error("communication timeout after {0}ms")]
    Timeout(u64),

    /// Invalid response from radio
    #[error("invalid response: {0}")]
    InvalidResponse(String),
}
