//! Radio panel UI component

use cat_mux::{is_virtual_port, sim_id_from_port, virtual_port_name, RadioHandle};
use cat_protocol::{OperatingMode, Protocol};

use crate::settings::ConfiguredRadio;

/// UI panel for a single radio
pub struct RadioPanel {
    /// Radio handle in the local multiplexer (None if pending connection)
    pub handle: Option<RadioHandle>,
    /// Display name
    pub name: String,
    /// Serial port (or "VSIM:..." for virtual radios)
    pub port: String,
    /// Protocol (for future use in protocol-specific UI)
    pub protocol: Protocol,
    /// Baud rate (for COM port radios)
    pub baud_rate: u32,
    /// CI-V address for Icom radios
    pub civ_address: Option<u8>,
    /// Is expanded in UI (for collapsible virtual radio controls)
    pub expanded: bool,
    /// Whether the port is unavailable (for restored radios)
    pub unavailable: bool,
    /// Current frequency in Hz (local state updated from MuxEvent)
    pub frequency_hz: Option<u64>,
    /// Current operating mode (local state updated from MuxEvent)
    pub mode: Option<OperatingMode>,
    /// Current PTT state (local state updated from MuxEvent)
    pub ptt: bool,
}

impl RadioPanel {
    /// Create a new radio panel from a saved configuration
    pub fn new_from_config(handle: Option<RadioHandle>, config: &ConfiguredRadio) -> Self {
        Self {
            handle,
            name: config.model_name.clone(),
            port: config.port.clone(),
            protocol: config.protocol,
            baud_rate: config.baud_rate,
            civ_address: config.civ_address,
            expanded: false,
            unavailable: false,
            frequency_hz: None,
            mode: None,
            ptt: false,
        }
    }

    /// Create a new COM port radio panel with explicit parameters
    pub fn new_com(
        handle: Option<RadioHandle>,
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
            unavailable: false,
            frequency_hz: None,
            mode: None,
            ptt: false,
        }
    }

    /// Create a new radio panel for a virtual radio
    pub fn new_virtual(
        handle: Option<RadioHandle>,
        name: String,
        protocol: Protocol,
        sim_id: String,
    ) -> Self {
        Self {
            handle,
            name,
            port: virtual_port_name(&sim_id),
            protocol,
            baud_rate: 0,
            civ_address: None,
            expanded: false,
            unavailable: false,
            frequency_hz: None,
            mode: None,
            ptt: false,
        }
    }

    /// Check if this is a virtual radio based on port name
    pub fn is_virtual(&self) -> bool {
        is_virtual_port(&self.port)
    }

    /// Get the simulation ID for virtual radios (derived from port name)
    pub fn sim_id(&self) -> Option<&str> {
        sim_id_from_port(&self.port)
    }
}
