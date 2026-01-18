//! CAT Protocol Library
//!
//! This crate provides parsing and encoding for the four major amateur radio
//! CAT (Computer Aided Transceiver) protocols:
//!
//! - **Yaesu CAT**: 5-byte binary command format with BCD frequency encoding
//! - **Icom CI-V**: Variable-length framed messages with address-based routing
//! - **Kenwood**: ASCII semicolon-terminated commands
//! - **Elecraft**: Kenwood-compatible with extended commands
//!
//! # Architecture
//!
//! Each protocol module provides:
//! - A streaming frame parser that handles partial data
//! - Command encoding to protocol-specific bytes
//! - Conversion to/from the normalized `RadioCommand` enum
//!
//! # Example
//!
//! ```rust
//! use cat_protocol::{RadioCommand, OperatingMode, ProtocolCodec, ToRadioCommand};
//! use cat_protocol::kenwood::{KenwoodCodec, KenwoodCommand};
//!
//! // Parse a Kenwood frequency command
//! let mut codec = KenwoodCodec::new();
//! codec.push_bytes(b"FA00014250000;");
//!
//! if let Some(cmd) = codec.next_command() {
//!     let radio_cmd = cmd.to_radio_command();
//!     assert!(matches!(radio_cmd, RadioCommand::SetFrequency { hz: 14_250_000 }));
//! }
//! ```

pub mod command;
pub mod elecraft;
pub mod error;
pub mod flex;
pub mod icom;
pub mod kenwood;
pub mod models;
pub mod yaesu;

pub use command::{OperatingMode, RadioCommand};
pub use error::{ParseError, ProtocolError};
pub use models::{ProtocolId, RadioCapabilities, RadioDatabase, RadioModel};

/// Identifies which CAT protocol variant a radio uses
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Protocol {
    /// Yaesu CAT protocol (5-byte binary commands)
    Yaesu,
    /// Icom CI-V protocol (framed variable-length messages)
    IcomCIV,
    /// Kenwood protocol (ASCII semicolon-terminated)
    Kenwood,
    /// Elecraft protocol (Kenwood-compatible with extensions)
    Elecraft,
    /// FlexRadio SmartSDR CAT protocol (Kenwood-compatible with ZZ extensions)
    FlexRadio,
}

impl Protocol {
    /// Returns a human-readable name for the protocol
    pub fn name(&self) -> &'static str {
        match self {
            Protocol::Yaesu => "Yaesu CAT",
            Protocol::IcomCIV => "Icom CI-V",
            Protocol::Kenwood => "Kenwood",
            Protocol::Elecraft => "Elecraft",
            Protocol::FlexRadio => "FlexRadio SmartSDR",
        }
    }
}

/// Trait for protocol codecs that can parse incoming data streams
pub trait ProtocolCodec {
    /// The command type produced by this codec
    type Command;

    /// Push raw bytes into the codec's buffer
    fn push_bytes(&mut self, data: &[u8]);

    /// Try to extract the next complete command from the buffer
    fn next_command(&mut self) -> Option<Self::Command>;

    /// Clear the internal buffer
    fn clear(&mut self);
}

/// Trait for commands that can be converted to normalized RadioCommand
pub trait ToRadioCommand {
    /// Convert this protocol-specific command to a normalized RadioCommand
    fn to_radio_command(&self) -> RadioCommand;
}

/// Trait for commands that can be created from a normalized RadioCommand
pub trait FromRadioCommand: Sized {
    /// Try to create a protocol-specific command from a RadioCommand
    fn from_radio_command(cmd: &RadioCommand) -> Option<Self>;
}

/// Trait for commands that can be encoded to bytes
pub trait EncodeCommand {
    /// Encode this command to its wire format
    fn encode(&self) -> Vec<u8>;
}
