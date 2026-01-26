//! Unified port information abstraction for real and virtual ports

use cat_detect::SerialPortInfo;
use cat_protocol::Protocol;

use crate::settings::VirtualPortConfig;

/// Unified port information for both real serial ports and virtual ports
#[derive(Debug, Clone)]
pub enum PortInfo {
    /// A real serial port
    Real(SerialPortInfo),
    /// A virtual port configured in settings
    Virtual(VirtualPortConfig),
}

impl PortInfo {
    /// Get the port name used for identification
    ///
    /// - Real ports: "/dev/ttyUSB0", "COM3", etc.
    /// - Virtual ports: "VSIM:<name>"
    pub fn port_name(&self) -> String {
        match self {
            PortInfo::Real(info) => info.port.clone(),
            PortInfo::Virtual(config) => format!("VSIM:{}", config.name),
        }
    }

    /// Get a display label for the port dropdown
    ///
    /// - Real ports: "ttyUSB0 (Product Name)" or just the port name
    /// - Virtual ports: "<name> [SIM - Protocol]"
    pub fn display_label(&self) -> String {
        match self {
            PortInfo::Real(info) => match &info.product {
                Some(product) => format!("{} ({})", info.port, product),
                None => info.port.clone(),
            },
            PortInfo::Virtual(config) => {
                format!("{} [SIM - {}]", config.name, config.protocol.name())
            }
        }
    }

    /// Check if this is a virtual port
    pub fn is_virtual(&self) -> bool {
        matches!(self, PortInfo::Virtual(_))
    }

    /// Get the protocol for virtual ports (None for real ports)
    pub fn virtual_protocol(&self) -> Option<Protocol> {
        match self {
            PortInfo::Real(_) => None,
            PortInfo::Virtual(config) => Some(config.protocol),
        }
    }
}
