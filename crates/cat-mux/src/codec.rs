//! Protocol Codec Wrapper
//!
//! This module provides a unified wrapper around the different protocol codecs
//! (Kenwood, Icom CI-V, Yaesu) for parsing radio data in the mux actor.

use cat_protocol::{
    icom::CivCodec, kenwood::KenwoodCodec, yaesu::YaesuCodec, Protocol, ProtocolCodec,
    RadioCommand, ToRadioCommand,
};

/// Boxed protocol codec for async operations
///
/// This enum wraps the different protocol codecs to provide a unified interface
/// for pushing bytes and extracting parsed commands.
pub enum ProtocolCodecBox {
    /// Kenwood/Elecraft/FlexRadio ASCII protocol codec
    Kenwood(KenwoodCodec),
    /// Icom CI-V binary protocol codec
    Icom(CivCodec),
    /// Yaesu binary protocol codec
    Yaesu(YaesuCodec),
}

impl ProtocolCodecBox {
    /// Create a new codec for the given protocol
    pub fn new(protocol: Protocol) -> Self {
        match protocol {
            Protocol::Kenwood | Protocol::Elecraft | Protocol::FlexRadio => {
                Self::Kenwood(KenwoodCodec::new())
            }
            Protocol::IcomCIV => Self::Icom(CivCodec::new()),
            Protocol::Yaesu | Protocol::YaesuAscii => Self::Yaesu(YaesuCodec::new()),
        }
    }

    /// Push raw bytes into the codec buffer
    pub fn push_bytes(&mut self, data: &[u8]) {
        match self {
            Self::Kenwood(c) => c.push_bytes(data),
            Self::Icom(c) => c.push_bytes(data),
            Self::Yaesu(c) => c.push_bytes(data),
        }
    }

    /// Extract the next parsed command, if available
    pub fn next_command(&mut self) -> Option<RadioCommand> {
        match self {
            Self::Kenwood(c) => c.next_command().map(|cmd| cmd.to_radio_command()),
            Self::Icom(c) => c.next_command().map(|cmd| cmd.to_radio_command()),
            Self::Yaesu(c) => c.next_command().map(|cmd| cmd.to_radio_command()),
        }
    }
}
