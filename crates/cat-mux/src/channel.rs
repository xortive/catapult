//! Radio channel types for multiplexer connections
//!
//! This module defines the metadata and channel structures for connecting
//! radios to the multiplexer. Both real (COM port) and virtual radios use
//! these types.

use cat_protocol::{Protocol, RadioModel};
use serde::{Deserialize, Serialize};

/// Type of radio connection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RadioType {
    /// Real hardware radio connected via COM/serial port
    Real,
    /// Virtual/simulated radio
    Virtual,
}

/// Metadata for a connected radio channel
#[derive(Debug, Clone)]
pub struct RadioChannelMeta {
    /// Protocol used by this radio
    pub protocol: Protocol,
    /// Whether this is a real or virtual radio
    pub radio_type: RadioType,
    /// Identified radio model (if known)
    pub model_info: Option<RadioModel>,
    /// Serial port name (for real radios)
    pub port_name: Option<String>,
    /// Simulation ID (for virtual radios)
    pub sim_id: Option<String>,
    /// Human-readable display name
    pub display_name: String,
    /// CI-V address (for Icom radios)
    pub civ_address: Option<u8>,
}

impl RadioChannelMeta {
    /// Create metadata for a real radio
    pub fn new_real(
        display_name: String,
        port_name: String,
        protocol: Protocol,
        civ_address: Option<u8>,
    ) -> Self {
        Self {
            protocol,
            radio_type: RadioType::Real,
            model_info: None,
            port_name: Some(port_name),
            sim_id: None,
            display_name,
            civ_address,
        }
    }

    /// Create metadata for a virtual radio
    pub fn new_virtual(display_name: String, sim_id: String, protocol: Protocol) -> Self {
        Self {
            protocol,
            radio_type: RadioType::Virtual,
            model_info: None,
            port_name: None,
            sim_id: Some(sim_id),
            display_name,
            civ_address: None,
        }
    }

    /// Check if this is a virtual/simulated radio
    pub fn is_simulated(&self) -> bool {
        self.radio_type == RadioType::Virtual
    }

    /// Update the model info after identification
    pub fn set_model(&mut self, model: RadioModel) {
        self.model_info = Some(model);
    }

    /// Update the display name
    pub fn set_display_name(&mut self, name: String) {
        self.display_name = name;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_real_radio_meta() {
        let meta = RadioChannelMeta::new_real(
            "IC-7300".to_string(),
            "/dev/ttyUSB0".to_string(),
            Protocol::IcomCIV,
            Some(0x94),
        );

        assert_eq!(meta.radio_type, RadioType::Real);
        assert!(!meta.is_simulated());
        assert_eq!(meta.port_name, Some("/dev/ttyUSB0".to_string()));
        assert_eq!(meta.civ_address, Some(0x94));
    }

    #[test]
    fn test_virtual_radio_meta() {
        let meta = RadioChannelMeta::new_virtual(
            "Virtual 1".to_string(),
            "sim-001".to_string(),
            Protocol::Kenwood,
        );

        assert_eq!(meta.radio_type, RadioType::Virtual);
        assert!(meta.is_simulated());
        assert_eq!(meta.sim_id, Some("sim-001".to_string()));
        assert!(meta.port_name.is_none());
    }
}
