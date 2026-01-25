//! Radio channel types for multiplexer connections
//!
//! This module defines the metadata and channel structures for connecting
//! radios to the multiplexer. Both real (COM port) and virtual radios use
//! these types.

use cat_protocol::{Protocol, RadioModel};

/// Prefix for virtual/simulated radio port names
pub const VIRTUAL_PORT_PREFIX: &str = "VSIM:";

/// Check if a port name represents a virtual radio
pub fn is_virtual_port(port_name: &str) -> bool {
    port_name.starts_with(VIRTUAL_PORT_PREFIX)
}

/// Create a virtual port name from a simulation ID
pub fn virtual_port_name(sim_id: &str) -> String {
    format!("{}{}", VIRTUAL_PORT_PREFIX, sim_id)
}

/// Extract simulation ID from a virtual port name
pub fn sim_id_from_port(port_name: &str) -> Option<&str> {
    port_name.strip_prefix(VIRTUAL_PORT_PREFIX)
}

/// Metadata for a connected radio channel
#[derive(Debug, Clone)]
pub struct RadioChannelMeta {
    /// Protocol used by this radio
    pub protocol: Protocol,
    /// Identified radio model (if known)
    pub model_info: Option<RadioModel>,
    /// Port name (real ports like "/dev/ttyUSB0" or virtual ports like "VSIM:sim-001")
    pub port_name: Option<String>,
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
            model_info: None,
            port_name: Some(port_name),
            display_name,
            civ_address,
        }
    }

    /// Create metadata for a virtual radio
    pub fn new_virtual(display_name: String, sim_id: String, protocol: Protocol) -> Self {
        Self {
            protocol,
            model_info: None,
            port_name: Some(virtual_port_name(&sim_id)),
            display_name,
            civ_address: None,
        }
    }

    /// Check if this is a virtual/simulated radio
    pub fn is_simulated(&self) -> bool {
        self.port_name
            .as_ref()
            .map(|p| is_virtual_port(p))
            .unwrap_or(false)
    }

    /// Get the simulation ID for virtual radios (derived from port name)
    pub fn sim_id(&self) -> Option<&str> {
        self.port_name.as_deref().and_then(sim_id_from_port)
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

        assert!(!meta.is_simulated());
        assert_eq!(meta.port_name, Some("/dev/ttyUSB0".to_string()));
        assert_eq!(meta.civ_address, Some(0x94));
        assert_eq!(meta.sim_id(), None);
    }

    #[test]
    fn test_virtual_radio_meta() {
        let meta = RadioChannelMeta::new_virtual(
            "Virtual 1".to_string(),
            "sim-001".to_string(),
            Protocol::Kenwood,
        );

        assert!(meta.is_simulated());
        assert_eq!(meta.port_name, Some("VSIM:sim-001".to_string()));
        assert_eq!(meta.sim_id(), Some("sim-001"));
    }

    #[test]
    fn test_virtual_port_helpers() {
        // Test is_virtual_port
        assert!(is_virtual_port("VSIM:sim-001"));
        assert!(is_virtual_port("VSIM:ic7300-sim"));
        assert!(!is_virtual_port("/dev/ttyUSB0"));
        assert!(!is_virtual_port("COM3"));
        assert!(!is_virtual_port(""));

        // Test virtual_port_name
        assert_eq!(virtual_port_name("sim-001"), "VSIM:sim-001");
        assert_eq!(virtual_port_name("ic7300-sim"), "VSIM:ic7300-sim");

        // Test sim_id_from_port
        assert_eq!(sim_id_from_port("VSIM:sim-001"), Some("sim-001"));
        assert_eq!(sim_id_from_port("VSIM:ic7300-sim"), Some("ic7300-sim"));
        assert_eq!(sim_id_from_port("/dev/ttyUSB0"), None);
        assert_eq!(sim_id_from_port("COM3"), None);
    }
}
