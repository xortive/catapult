//! Serial port scanner for radio detection
//!
//! This module provides the main entry point for discovering and
//! identifying radios connected to the system.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use cat_protocol::{Protocol, RadioModel};
use serialport::{available_ports, SerialPortType};
use tokio_serial::SerialPortBuilderExt;
use tracing::{debug, info, warn};

use crate::error::DetectError;
use crate::probe::{ProbeConfig, RadioProber};
use crate::usb_ids::{self, PortClassification};

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
    /// Adapter type (FTDI, CP210x, etc.)
    pub adapter_type: Option<String>,
    /// Port classification for safe probing decisions
    pub classification: PortClassification,
    /// Human-readable hint for display (e.g., "Icom USB", "SmartSDR", "FTDI")
    pub classification_hint: Option<String>,
}

impl SerialPortInfo {
    /// Create from serialport crate's port info
    fn from_serialport(name: String, port_type: &SerialPortType) -> Self {
        match port_type {
            SerialPortType::UsbPort(usb) => {
                let (classification, hint) =
                    usb_ids::classify_port(Some(usb.vid), Some(usb.pid), &name);
                Self {
                    port: name,
                    vid: Some(usb.vid),
                    pid: Some(usb.pid),
                    serial_number: usb.serial_number.clone(),
                    manufacturer: usb.manufacturer.clone(),
                    product: usb.product.clone(),
                    adapter_type: usb_ids::adapter_name(usb.vid).map(String::from),
                    classification,
                    classification_hint: hint.map(String::from),
                }
            }
            _ => {
                let (classification, hint) = usb_ids::classify_port(None, None, &name);
                Self {
                    port: name,
                    vid: None,
                    pid: None,
                    serial_number: None,
                    manufacturer: None,
                    product: None,
                    adapter_type: None,
                    classification,
                    classification_hint: hint.map(String::from),
                }
            }
        }
    }

    /// Check if this is a known USB serial adapter
    pub fn is_known_adapter(&self) -> bool {
        match (self.vid, self.pid) {
            (Some(vid), Some(pid)) => usb_ids::is_known_serial_adapter(vid, pid),
            _ => false,
        }
    }

    /// Check if this is a known radio with built-in USB
    pub fn known_radio(&self) -> Option<&'static str> {
        match (self.vid, self.pid) {
            (Some(vid), Some(pid)) => usb_ids::is_known_radio_usb(vid, pid),
            _ => None,
        }
    }
}

/// A detected radio
#[derive(Debug, Clone)]
pub struct DetectedRadio {
    /// Serial port name
    pub port: String,
    /// Detected protocol
    pub protocol: Protocol,
    /// Radio model (if identified)
    pub model: Option<RadioModel>,
    /// Port information
    pub port_info: SerialPortInfo,
    /// CI-V address (for Icom radios)
    pub civ_address: Option<u8>,
    /// Baud rate used for detection
    pub baud_rate: u32,
    /// When this radio was detected
    pub detected_at: Instant,
}

impl DetectedRadio {
    /// Get a display name for the radio
    pub fn model_name(&self) -> String {
        if let Some(ref model) = self.model {
            format!("{} {}", model.manufacturer, model.model)
        } else if let Some(known) = self.port_info.known_radio() {
            known.to_string()
        } else {
            format!("{} radio", self.protocol.name())
        }
    }
}

/// Serial port scanner configuration
#[derive(Debug, Clone)]
pub struct ScannerConfig {
    /// Probe configuration
    pub probe: ProbeConfig,
    /// Whether to filter by known USB adapters
    pub filter_known_adapters: bool,
    /// Additional baud rates to try
    pub baud_rates: Vec<u32>,
    /// Skip ports matching these patterns
    pub skip_patterns: Vec<String>,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            probe: ProbeConfig::default(),
            filter_known_adapters: false,
            baud_rates: vec![38400, 19200, 9600, 4800, 115200],
            skip_patterns: vec![
                // Bluetooth ports on macOS
                "Bluetooth".to_string(),
                // Debug/logging ports
                "debug".to_string(),
            ],
        }
    }
}

/// Serial port scanner
pub struct PortScanner {
    config: ScannerConfig,
    prober: RadioProber,
    /// Cache of previously detected radios
    detected_cache: HashMap<String, DetectedRadio>,
}

impl PortScanner {
    /// Create a new scanner with default configuration
    pub fn new() -> Self {
        Self {
            config: ScannerConfig::default(),
            prober: RadioProber::new(),
            detected_cache: HashMap::new(),
        }
    }

    /// Sort ports by classification (known radios first, unknown last)
    pub fn sort_by_classification(ports: &mut [SerialPortInfo]) {
        ports.sort_by_key(|p| p.classification);
    }

    /// Create a scanner with custom configuration
    pub fn with_config(config: ScannerConfig) -> Self {
        Self {
            prober: RadioProber::with_config(config.probe.clone()),
            config,
            detected_cache: HashMap::new(),
        }
    }

    /// Enumerate all available serial ports
    pub fn enumerate_ports(&self) -> Result<Vec<SerialPortInfo>, DetectError> {
        debug!("Starting port enumeration...");
        let ports = available_ports().map_err(|e| DetectError::EnumerationFailed(e.to_string()))?;

        let result: Vec<_> = ports
            .into_iter()
            .map(|p| SerialPortInfo::from_serialport(p.port_name, &p.port_type))
            .filter(|p| !self.should_skip_port(p))
            .collect();

        for port in &result {
            match (port.vid, port.pid) {
                (Some(vid), Some(pid)) => {
                    debug!(
                        "Found port: {} (VID:{:04X} PID:{:04X}) - {:?}",
                        port.port, vid, pid, port.classification
                    );
                }
                _ => {
                    debug!(
                        "Found port: {} (no USB info) - {:?}",
                        port.port, port.classification
                    );
                }
            }
        }

        debug!("Enumerated {} ports total", result.len());
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

    /// Scan all ports and detect radios
    ///
    /// Only probes ports classified as safe (KnownRadio or VirtualPort).
    /// Ports with generic adapters or unknown devices are skipped to avoid
    /// disrupting other equipment.
    pub async fn scan(&mut self) -> Vec<DetectedRadio> {
        info!("Starting radio scan...");
        let ports = match self.enumerate_ports() {
            Ok(p) => p,
            Err(e) => {
                warn!("Failed to enumerate ports: {}", e);
                return vec![];
            }
        };

        info!("Scanning {} ports for radios", ports.len());
        let mut detected = Vec::new();

        for port_info in ports {
            // Only auto-probe ports that are safe to probe
            if !port_info.classification.is_safe_to_probe() {
                debug!(
                    "Skipping probe of {}: {:?} (not safe to auto-probe)",
                    port_info.port, port_info.classification
                );
                continue;
            }

            if self.config.filter_known_adapters && !port_info.is_known_adapter() {
                debug!("Skipping unknown adapter: {}", port_info.port);
                continue;
            }

            match self.probe_port(&port_info).await {
                Some(radio) => {
                    info!(
                        "Detected {} on {} at {} baud",
                        radio.model_name(),
                        radio.port,
                        radio.baud_rate
                    );
                    self.detected_cache
                        .insert(radio.port.clone(), radio.clone());
                    detected.push(radio);
                }
                None => {
                    debug!("No radio found on {}", port_info.port);
                }
            }
        }

        if detected.is_empty() {
            info!("Scan complete - no radios found");
        } else {
            info!("Scan complete - found {} radio(s)", detected.len());
        }

        detected
    }

    /// Probe a specific port for a radio
    async fn probe_port(&self, port_info: &SerialPortInfo) -> Option<DetectedRadio> {
        debug!(
            "Probing port {} for radios (trying {} baud rates)...",
            port_info.port,
            self.config.baud_rates.len()
        );
        for &baud in &self.config.baud_rates {
            if let Some(radio) = self.probe_at_baud(port_info, baud).await {
                return Some(radio);
            }
        }
        debug!("No response from {} at any baud rate", port_info.port);
        None
    }

    /// Probe at a specific baud rate
    async fn probe_at_baud(&self, port_info: &SerialPortInfo, baud: u32) -> Option<DetectedRadio> {
        debug!("Probing {} at {} baud", port_info.port, baud);

        let mut stream = match tokio_serial::new(&port_info.port, baud)
            .timeout(Duration::from_millis(100))
            .open_native_async()
        {
            Ok(s) => s,
            Err(e) => {
                debug!("Failed to open {}: {}", port_info.port, e);
                return None;
            }
        };

        // Give the port a moment to settle
        tokio::time::sleep(Duration::from_millis(50)).await;

        let result = self.prober.probe(&mut stream).await?;

        Some(DetectedRadio {
            port: port_info.port.clone(),
            protocol: result.protocol,
            model: result.model,
            port_info: port_info.clone(),
            civ_address: result.address,
            baud_rate: baud,
            detected_at: Instant::now(),
        })
    }

    /// Get cached detection results
    pub fn cached_radios(&self) -> impl Iterator<Item = &DetectedRadio> {
        self.detected_cache.values()
    }

    /// Clear detection cache
    pub fn clear_cache(&mut self) {
        self.detected_cache.clear();
    }

    /// Quick scan - only probe at common baud rates
    pub async fn quick_scan(&mut self) -> Vec<DetectedRadio> {
        let original_rates = self.config.baud_rates.clone();
        self.config.baud_rates = vec![38400, 9600, 115200];

        let result = self.scan().await;

        self.config.baud_rates = original_rates;
        result
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
        assert_eq!(info.adapter_type.as_deref(), Some("FTDI"));
        assert!(info.is_known_adapter());
        // FTDI is a known adapter, not a known radio
        assert_eq!(info.classification, PortClassification::KnownAdapter);
        assert_eq!(info.classification_hint.as_deref(), Some("FTDI"));
        assert!(!info.classification.is_safe_to_probe());
    }

    #[test]
    fn test_serial_port_info_from_icom() {
        let usb_info = SerialPortType::UsbPort(UsbPortInfo {
            vid: 0x0C26, // Icom VID
            pid: 0x0036, // IC-7300
            serial_number: None,
            manufacturer: Some("Icom Inc.".to_string()),
            product: Some("IC-7300".to_string()),
        });

        let info = SerialPortInfo::from_serialport("/dev/ttyACM0".to_string(), &usb_info);

        assert_eq!(info.classification, PortClassification::KnownRadio);
        assert_eq!(info.classification_hint.as_deref(), Some("Icom USB"));
        assert!(info.classification.is_safe_to_probe());
    }

    #[test]
    fn test_detected_radio_name() {
        let radio = DetectedRadio {
            port: "/dev/ttyUSB0".to_string(),
            protocol: Protocol::Kenwood,
            model: None,
            port_info: SerialPortInfo {
                port: "/dev/ttyUSB0".to_string(),
                vid: None,
                pid: None,
                serial_number: None,
                manufacturer: None,
                product: None,
                adapter_type: None,
                classification: PortClassification::Unknown,
                classification_hint: None,
            },
            civ_address: None,
            baud_rate: 38400,
            detected_at: Instant::now(),
        };

        assert_eq!(radio.model_name(), "Kenwood radio");
    }

    #[test]
    fn test_sort_by_classification() {
        let mut ports = vec![
            SerialPortInfo {
                port: "/dev/ttyUSB0".to_string(),
                vid: None,
                pid: None,
                serial_number: None,
                manufacturer: None,
                product: None,
                adapter_type: None,
                classification: PortClassification::Unknown,
                classification_hint: None,
            },
            SerialPortInfo {
                port: "/dev/ttyACM0".to_string(),
                vid: Some(0x0C26),
                pid: Some(0x0036),
                serial_number: None,
                manufacturer: None,
                product: None,
                adapter_type: Some("Icom USB".to_string()),
                classification: PortClassification::KnownRadio,
                classification_hint: Some("Icom USB".to_string()),
            },
            SerialPortInfo {
                port: "/dev/ttyUSB1".to_string(),
                vid: Some(0x0403),
                pid: Some(0x6001),
                serial_number: None,
                manufacturer: None,
                product: None,
                adapter_type: Some("FTDI".to_string()),
                classification: PortClassification::KnownAdapter,
                classification_hint: Some("FTDI".to_string()),
            },
        ];

        PortScanner::sort_by_classification(&mut ports);

        // Should be sorted: KnownRadio, KnownAdapter, Unknown
        assert_eq!(ports[0].classification, PortClassification::KnownRadio);
        assert_eq!(ports[1].classification, PortClassification::KnownAdapter);
        assert_eq!(ports[2].classification, PortClassification::Unknown);
    }
}
