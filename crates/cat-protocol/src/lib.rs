//! CAT Protocol Library
//!
//! This crate provides parsing and encoding for amateur radio
//! CAT (Computer Aided Transceiver) protocols:
//!
//! - **Yaesu CAT**: 5-byte binary command format with BCD frequency encoding (FT-817/857/897)
//! - **Yaesu ASCII**: ASCII semicolon-terminated commands (FT-991/FTDX series)
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
pub mod display;
pub mod elecraft;
pub mod error;
pub mod flex;
pub mod icom;
pub mod kenwood;
pub mod models;
pub mod yaesu;
pub mod yaesu_ascii;

pub use command::{OperatingMode, RadioCommand};
pub use error::{ParseError, ProtocolError};
pub use models::{ProtocolId, RadioCapabilities, RadioDatabase, RadioModel};

/// Identifies which CAT protocol variant a radio uses
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Protocol {
    /// Yaesu CAT protocol (5-byte binary commands for FT-817/857/897)
    Yaesu,
    /// Yaesu ASCII protocol (semicolon-terminated for FT-991/FTDX series)
    YaesuAscii,
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
            Protocol::YaesuAscii => "Yaesu ASCII",
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

/// Object-safe trait for codecs that parse raw bytes into [`RadioCommand`]s
///
/// Unlike [`ProtocolCodec`], this trait returns the normalized `RadioCommand`
/// directly, making it object-safe and usable as `Box<dyn RadioCodec>`.
pub trait RadioCodec: Send + Sync {
    /// Push raw bytes into the codec's buffer
    fn push_bytes(&mut self, data: &[u8]);

    /// Try to extract the next complete command from the buffer
    fn next_command(&mut self) -> Option<RadioCommand>;

    /// Clear the internal buffer
    fn clear(&mut self);
}

/// Create a codec for the given protocol
pub fn create_radio_codec(protocol: Protocol) -> Box<dyn RadioCodec> {
    match protocol {
        Protocol::Kenwood | Protocol::Elecraft | Protocol::FlexRadio => {
            Box::new(kenwood::KenwoodCodec::new())
        }
        Protocol::IcomCIV => Box::new(icom::CivCodec::new()),
        Protocol::Yaesu | Protocol::YaesuAscii => Box::new(yaesu::YaesuCodec::new()),
    }
}
