//! Amplifier channel types for multiplexer connections
//!
//! This module defines the metadata and channel structures for connecting
//! amplifiers to the multiplexer. Supports both real (COM port) and virtual
//! amplifiers.

use cat_protocol::Protocol;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Type of amplifier connection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmplifierType {
    /// Real hardware amplifier connected via COM/serial port
    Real,
    /// Virtual/simulated amplifier for testing
    Virtual,
}

/// Metadata for an amplifier channel
#[derive(Debug, Clone)]
pub struct AmplifierChannelMeta {
    /// Protocol used to communicate with this amplifier
    pub protocol: Protocol,
    /// Whether this is a real or virtual amplifier
    pub amp_type: AmplifierType,
    /// Serial port name (for real amplifiers)
    pub port_name: Option<String>,
    /// CI-V address (for Icom amplifiers)
    pub civ_address: Option<u8>,
    /// Baud rate for serial communication
    pub baud_rate: u32,
}

impl AmplifierChannelMeta {
    /// Create metadata for a real amplifier
    pub fn new_real(
        port_name: String,
        protocol: Protocol,
        baud_rate: u32,
        civ_address: Option<u8>,
    ) -> Self {
        Self {
            protocol,
            amp_type: AmplifierType::Real,
            port_name: Some(port_name),
            civ_address,
            baud_rate,
        }
    }

    /// Create metadata for a virtual amplifier
    pub fn new_virtual(protocol: Protocol, civ_address: Option<u8>) -> Self {
        Self {
            protocol,
            amp_type: AmplifierType::Virtual,
            port_name: None,
            civ_address,
            baud_rate: 0, // Not used for virtual
        }
    }

    /// Check if this is a virtual/simulated amplifier
    pub fn is_simulated(&self) -> bool {
        self.amp_type == AmplifierType::Virtual
    }
}

/// Bidirectional amplifier channel
///
/// The multiplexer sends translated commands to the amplifier through `command_tx`,
/// and receives responses through `response_rx`.
pub struct AmplifierChannel {
    /// Metadata about this amplifier
    pub meta: AmplifierChannelMeta,
    /// Sender for commands to the amplifier (mux -> amp)
    pub command_tx: mpsc::Sender<Vec<u8>>,
    /// Receiver for responses from the amplifier (amp -> mux)
    pub response_rx: mpsc::Receiver<Vec<u8>>,
}

impl std::fmt::Debug for AmplifierChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AmplifierChannel")
            .field("meta", &self.meta)
            .field("command_tx", &"<sender>")
            .field("response_rx", &"<receiver>")
            .finish()
    }
}

impl AmplifierChannel {
    /// Create a new amplifier channel
    pub fn new(
        meta: AmplifierChannelMeta,
        command_tx: mpsc::Sender<Vec<u8>>,
        response_rx: mpsc::Receiver<Vec<u8>>,
    ) -> Self {
        Self {
            meta,
            command_tx,
            response_rx,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amplifier_meta_real() {
        let meta = AmplifierChannelMeta::new_real(
            "/dev/ttyUSB1".to_string(),
            Protocol::Kenwood,
            38400,
            None,
        );

        assert_eq!(meta.amp_type, AmplifierType::Real);
        assert!(!meta.is_simulated());
        assert_eq!(meta.port_name, Some("/dev/ttyUSB1".to_string()));
        assert_eq!(meta.baud_rate, 38400);
    }

    #[test]
    fn test_amplifier_meta_virtual() {
        let meta = AmplifierChannelMeta::new_virtual(Protocol::IcomCIV, Some(0x94));

        assert_eq!(meta.amp_type, AmplifierType::Virtual);
        assert!(meta.is_simulated());
        assert!(meta.port_name.is_none());
        assert_eq!(meta.civ_address, Some(0x94));
    }
}
