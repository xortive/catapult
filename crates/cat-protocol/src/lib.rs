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
//! - Conversion to/from normalized `RadioRequest` and `RadioResponse` enums
//!
//! The same protocol bytes can mean different things based on direction:
//! - `FA00014250000;` FROM radio = Response (frequency report)
//! - `FA00014250000;` TO radio = Request (set frequency)
//! - `FA;` FROM amplifier = Request (query frequency)
//!
//! # Example
//!
//! ```rust
//! use cat_protocol::{RadioResponse, OperatingMode, ProtocolCodec, ToRadioResponse};
//! use cat_protocol::kenwood::{KenwoodCodec, KenwoodCommand};
//!
//! // Parse a Kenwood frequency response from a radio
//! let mut codec = KenwoodCodec::new();
//! codec.push_bytes(b"FA00014250000;");
//!
//! if let Some(cmd) = codec.next_command() {
//!     let response = cmd.to_radio_response();
//!     assert!(matches!(response, RadioResponse::Frequency { hz: 14_250_000 }));
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

pub use command::{OperatingMode, RadioRequest, RadioResponse, Vfo};
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

    /// Try to extract the next complete command along with its raw bytes
    ///
    /// This is useful for traffic monitoring where we want to show the exact
    /// bytes that were parsed for each command.
    fn next_command_with_bytes(&mut self) -> Option<(Self::Command, Vec<u8>)>;

    /// Clear the internal buffer
    fn clear(&mut self);
}

/// Parse protocol command as a response (radio → mux)
pub trait ToRadioResponse {
    /// Convert this protocol-specific command to a RadioResponse
    fn to_radio_response(&self) -> RadioResponse;
}

/// Parse protocol command as a request (amplifier → mux)
pub trait ToRadioRequest {
    /// Convert this protocol-specific command to a RadioRequest
    fn to_radio_request(&self) -> RadioRequest;
}

/// Encode request to protocol bytes (mux → radio)
pub trait FromRadioRequest: Sized {
    /// Try to create a protocol-specific command from a RadioRequest
    fn from_radio_request(req: &RadioRequest) -> Option<Self>;
}

/// Encode response to protocol bytes (mux → amplifier)
pub trait FromRadioResponse: Sized {
    /// Try to create a protocol-specific command from a RadioResponse
    fn from_radio_response(resp: &RadioResponse) -> Option<Self>;
}

/// Trait for commands that can be encoded to bytes
pub trait EncodeCommand {
    /// Encode this command to its wire format
    fn encode(&self) -> Vec<u8>;
}

/// Object-safe trait for codecs that parse raw bytes into [`RadioResponse`]s
///
/// Unlike [`ProtocolCodec`], this trait returns the normalized `RadioResponse`
/// directly, making it object-safe and usable as `Box<dyn RadioCodec>`.
///
/// This is used for data FROM radios (radio → mux direction).
pub trait RadioCodec: Send + Sync {
    /// Push raw bytes into the codec's buffer
    fn push_bytes(&mut self, data: &[u8]);

    /// Try to extract the next complete response from the buffer
    fn next_response(&mut self) -> Option<RadioResponse>;

    /// Try to extract the next complete response along with its raw bytes
    fn next_response_with_bytes(&mut self) -> Option<(RadioResponse, Vec<u8>)>;

    /// Try to extract the next complete request from the buffer
    /// (used for parsing amplifier → mux direction)
    fn next_request(&mut self) -> Option<RadioRequest>;

    /// Try to extract the next complete request along with its raw bytes
    fn next_request_with_bytes(&mut self) -> Option<(RadioRequest, Vec<u8>)>;

    /// Clear the internal buffer
    fn clear(&mut self);
}

/// Implements [`RadioCodec`] for a type that already implements [`ProtocolCodec`]
/// with a command type implementing [`ToRadioResponse`] and [`ToRadioRequest`].
#[macro_export]
macro_rules! impl_radio_codec {
    ($codec:ty) => {
        impl $crate::RadioCodec for $codec {
            fn push_bytes(&mut self, data: &[u8]) {
                $crate::ProtocolCodec::push_bytes(self, data);
            }

            fn next_response(&mut self) -> Option<$crate::RadioResponse> {
                $crate::ProtocolCodec::next_command(self).map(|cmd| cmd.to_radio_response())
            }

            fn next_response_with_bytes(&mut self) -> Option<($crate::RadioResponse, Vec<u8>)> {
                $crate::ProtocolCodec::next_command_with_bytes(self)
                    .map(|(cmd, bytes)| (cmd.to_radio_response(), bytes))
            }

            fn next_request(&mut self) -> Option<$crate::RadioRequest> {
                $crate::ProtocolCodec::next_command(self).map(|cmd| cmd.to_radio_request())
            }

            fn next_request_with_bytes(&mut self) -> Option<($crate::RadioRequest, Vec<u8>)> {
                $crate::ProtocolCodec::next_command_with_bytes(self)
                    .map(|(cmd, bytes)| (cmd.to_radio_request(), bytes))
            }

            fn clear(&mut self) {
                $crate::ProtocolCodec::clear(self);
            }
        }
    };
}

/// Create a codec for the given protocol
pub fn create_radio_codec(protocol: Protocol) -> Box<dyn RadioCodec> {
    match protocol {
        Protocol::Kenwood | Protocol::Elecraft | Protocol::FlexRadio => {
            Box::new(kenwood::KenwoodCodec::new())
        }
        Protocol::IcomCIV => Box::new(icom::CivCodec::new()),
        Protocol::Yaesu => Box::new(yaesu::YaesuCodec::new()),
        Protocol::YaesuAscii => Box::new(yaesu_ascii::YaesuAsciiCodec::new()),
    }
}
