//! Serial port scanner
//!
//! This module provides serial port enumeration.

use serialport::{available_ports, SerialPortType};
use tracing::info;

use crate::error::DetectError;

/// Information about a serial port
#[derive(Debug, Clone)]
pub struct SerialPortInfo {
    /// Port name (e.g., /dev/ttyUSB0, COM3)
    pub port: String,
    /// USB Vendor ID (if USB)
    pub vid: Option<u16>,
    /// USB Product ID (if USB)
    pub pid: Option<u16>,
    /// USB serial number (if available)
    pub serial_number: Option<String>,
    /// USB manufacturer string
    pub manufacturer: Option<String>,
    /// USB product string
    pub product: Option<String>,
}

impl SerialPortInfo {
    /// Create from serialport crate's port info
    fn from_serialport(name: String, port_type: &SerialPortType) -> Self {
        match port_type {
            SerialPortType::UsbPort(usb) => Self {
                port: name,
                vid: Some(usb.vid),
                pid: Some(usb.pid),
                serial_number: usb.serial_number.clone(),
                manufacturer: usb.manufacturer.clone(),
                product: usb.product.clone(),
            },
            _ => Self {
                port: name,
                vid: None,
                pid: None,
                serial_number: None,
                manufacturer: None,
                product: None,
            },
        }
    }
}

/// Serial port scanner configuration
#[derive(Debug, Clone, Default)]
pub struct ScannerConfig {
    /// Skip ports matching these patterns
    pub skip_patterns: Vec<String>,
}

/// Serial port scanner
pub struct PortScanner {
    config: ScannerConfig,
}

impl PortScanner {
    /// Create a new scanner with default configuration
    pub fn new() -> Self {
        Self {
            config: ScannerConfig {
                skip_patterns: vec![
                    // Bluetooth ports on macOS
                    "Bluetooth".to_string(),
                    // Debug/logging ports
                    "debug".to_string(),
                ],
            },
        }
    }

    /// Create a scanner with custom configuration
    pub fn with_config(config: ScannerConfig) -> Self {
        Self { config }
    }

    /// Enumerate all available serial ports
    pub fn enumerate_ports(&self) -> Result<Vec<SerialPortInfo>, DetectError> {
        info!("Enumerating serial ports...");
        let ports = available_ports().map_err(|e| DetectError::EnumerationFailed(e.to_string()))?;

        let result: Vec<_> = ports
            .into_iter()
            .map(|p| SerialPortInfo::from_serialport(p.port_name, &p.port_type))
            .filter(|p| !self.should_skip_port(p))
            .collect();

        if result.is_empty() {
            info!("No serial ports found");
        } else {
            info!("Found {} serial port(s)", result.len());
            for port in &result {
                let desc = port.product.as_deref().unwrap_or("Unknown");
                info!("  {} - {}", port.port, desc);
            }
        }

        Ok(result)
    }

    /// Check if a port should be skipped
    fn should_skip_port(&self, port: &SerialPortInfo) -> bool {
        for pattern in &self.config.skip_patterns {
            if port.port.contains(pattern) {
                return true;
            }
        }
        false
    }
}

impl Default for PortScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serialport::UsbPortInfo;

    #[test]
    fn test_serial_port_info_from_usb() {
        let usb_info = SerialPortType::UsbPort(UsbPortInfo {
            vid: 0x0403,
            pid: 0x6001,
            serial_number: Some("12345".to_string()),
            manufacturer: Some("FTDI".to_string()),
            product: Some("FT232R".to_string()),
        });

        let info = SerialPortInfo::from_serialport("/dev/ttyUSB0".to_string(), &usb_info);

        assert_eq!(info.vid, Some(0x0403));
        assert_eq!(info.pid, Some(0x6001));
        assert_eq!(info.product.as_deref(), Some("FT232R"));
    }
}
