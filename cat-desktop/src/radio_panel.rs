//! Radio panel UI component

use cat_detect::DetectedRadio;
use cat_mux::RadioHandle;
use cat_protocol::Protocol;

use crate::settings::ConfiguredRadio;

/// Type of connection for a radio
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioConnectionType {
    /// Physical radio connected via COM/serial port
    ComPort,
    /// Virtual/simulated radio
    Virtual,
}

impl RadioConnectionType {
    /// Get the badge text for display
    pub fn badge(&self) -> &'static str {
        match self {
            RadioConnectionType::ComPort => "[COM]",
            RadioConnectionType::Virtual => "[VRT]",
        }
    }
}

/// UI panel for a single radio
pub struct RadioPanel {
    /// Radio handle in the multiplexer
    pub handle: RadioHandle,
    /// Display name
    pub name: String,
    /// Serial port (or "VRT" for virtual)
    pub port: String,
    /// Protocol (for future use in protocol-specific UI)
    pub protocol: Protocol,
    /// Baud rate (for COM port radios)
    pub baud_rate: u32,
    /// CI-V address for Icom radios
    pub civ_address: Option<u8>,
    /// Is expanded in UI (for collapsible virtual radio controls)
    pub expanded: bool,
    /// Connection type (COM port or Virtual)
    pub connection_type: RadioConnectionType,
    /// Simulation radio ID (only for Virtual radios)
    pub sim_radio_id: Option<String>,
    /// Whether the port is unavailable (for restored radios)
    pub unavailable: bool,
}

impl RadioPanel {
    /// Create a new radio panel from a detected radio (COM port)
    pub fn new(handle: RadioHandle, detected: &DetectedRadio) -> Self {
        Self {
            handle,
            name: detected.model_name(),
            port: detected.port.clone(),
            protocol: detected.protocol,
            baud_rate: detected.baud_rate,
            civ_address: detected.civ_address,
            expanded: false,
            connection_type: RadioConnectionType::ComPort,
            sim_radio_id: None,
            unavailable: false,
        }
    }

    /// Create a new radio panel from a saved configuration
    pub fn new_from_config(handle: RadioHandle, config: &ConfiguredRadio) -> Self {
        Self {
            handle,
            name: config.model_name.clone(),
            port: config.port.clone(),
            protocol: config.protocol,
            baud_rate: config.baud_rate,
            civ_address: config.civ_address,
            expanded: false,
            connection_type: RadioConnectionType::ComPort,
            sim_radio_id: None,
            unavailable: false,
        }
    }

    /// Create a new COM port radio panel with explicit parameters
    pub fn new_com(
        handle: RadioHandle,
        name: String,
        port: String,
        protocol: Protocol,
        baud_rate: u32,
        civ_address: Option<u8>,
    ) -> Self {
        Self {
            handle,
            name,
            port,
            protocol,
            baud_rate,
            civ_address,
            expanded: false,
            connection_type: RadioConnectionType::ComPort,
            sim_radio_id: None,
            unavailable: false,
        }
    }

    /// Create a new radio panel for a virtual radio
    pub fn new_virtual(
        handle: RadioHandle,
        name: String,
        protocol: Protocol,
        sim_id: String,
    ) -> Self {
        Self {
            handle,
            name,
            port: "VRT".to_string(),
            protocol,
            baud_rate: 0,
            civ_address: None,
            expanded: false,
            connection_type: RadioConnectionType::Virtual,
            sim_radio_id: Some(sim_id),
            unavailable: false,
        }
    }
}
