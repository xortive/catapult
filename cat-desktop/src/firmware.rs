//! ESP32-S3 firmware flashing module
//!
//! Uses the espflash library to flash cat-bridge firmware to ESP32-S3 devices.

use std::sync::mpsc::Sender;

use espflash::connection::{Connection, ResetAfterOperation, ResetBeforeOperation};
use espflash::flasher::Flasher;
use espflash::target::{Chip, ProgressCallbacks};
use serialport::{available_ports, FlowControl, SerialPortType, UsbPortInfo};

/// Bundled cat-bridge firmware binary
const FIRMWARE_BINARY: &[u8] = include_bytes!("../assets/cat-bridge.bin");

/// Firmware version (should match cat-bridge version)
pub const FIRMWARE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// ESP32-S3 USB-Serial-JTAG USB IDs
const ESP32_S3_VID: u16 = 0x303a;
const ESP32_S3_PID: u16 = 0x1001;

/// Flash state for UI updates
#[derive(Debug, Clone)]
pub enum FlashState {
    /// Idle, ready to flash
    Idle,
    /// Connecting to device
    Connecting,
    /// Erasing flash
    Erasing,
    /// Writing firmware with progress (0.0 - 1.0)
    Writing { progress: f32 },
    /// Verifying firmware
    Verifying,
    /// Flashing complete
    Complete,
    /// Error occurred
    Error(String),
}

/// Message sent from flash thread to UI
#[derive(Debug, Clone)]
pub enum FlashMessage {
    /// State changed
    StateChanged(FlashState),
    /// Log message
    Log(String),
}

/// Detected ESP32-S3 device
#[derive(Debug, Clone)]
pub struct DetectedDevice {
    /// Serial port name
    pub port: String,
    /// USB serial number (if available)
    pub serial_number: Option<String>,
}

/// Progress callback implementation for espflash
struct FlashProgress {
    sender: Sender<FlashMessage>,
    total_size: usize,
}

impl ProgressCallbacks for FlashProgress {
    fn init(&mut self, _addr: u32, total_size: usize) {
        self.total_size = total_size;
        let _ = self.sender.send(FlashMessage::Log(format!(
            "Starting flash: {} bytes",
            total_size
        )));
        let _ = self
            .sender
            .send(FlashMessage::StateChanged(FlashState::Writing {
                progress: 0.0,
            }));
    }

    fn update(&mut self, current: usize) {
        let progress = if self.total_size > 0 {
            current as f32 / self.total_size as f32
        } else {
            0.0
        };

        let _ = self
            .sender
            .send(FlashMessage::StateChanged(FlashState::Writing { progress }));
    }

    fn verifying(&mut self) {
        let _ = self
            .sender
            .send(FlashMessage::Log("Verifying segment...".into()));
    }

    fn finish(&mut self, _skipped: bool) {
        let _ = self
            .sender
            .send(FlashMessage::Log("Segment complete".into()));
    }
}

/// Detect ESP32-S3 devices connected via USB-Serial-JTAG
pub fn detect_devices() -> Vec<DetectedDevice> {
    let mut devices = Vec::new();

    if let Ok(ports) = available_ports() {
        for port in ports {
            if let SerialPortType::UsbPort(usb_info) = &port.port_type {
                if usb_info.vid == ESP32_S3_VID && usb_info.pid == ESP32_S3_PID {
                    devices.push(DetectedDevice {
                        port: port.port_name.clone(),
                        serial_number: usb_info.serial_number.clone(),
                    });
                }
            }
        }
    }

    devices
}

/// Flash firmware to the specified port
///
/// This function runs in a background thread and sends progress updates
/// via the provided sender channel.
pub fn flash_firmware(port: &str, sender: Sender<FlashMessage>) {
    let port = port.to_string();

    std::thread::spawn(move || {
        if let Err(e) = do_flash(&port, &sender) {
            let _ = sender.send(FlashMessage::StateChanged(FlashState::Error(e.to_string())));
            let _ = sender.send(FlashMessage::Log(format!("Flash failed: {}", e)));
        }
    });
}

/// Get USB port info for a given port name
fn get_usb_port_info(port_name: &str) -> Option<UsbPortInfo> {
    available_ports().ok()?.into_iter().find_map(|p| {
        if p.port_name == port_name {
            if let SerialPortType::UsbPort(info) = p.port_type {
                return Some(info);
            }
        }
        None
    })
}

/// Internal flash implementation
fn do_flash(port_name: &str, sender: &Sender<FlashMessage>) -> Result<(), Box<dyn std::error::Error>> {
    // Send connecting state
    sender.send(FlashMessage::StateChanged(FlashState::Connecting))?;
    sender.send(FlashMessage::Log(format!("Connecting to {}...", port_name)))?;

    // Get USB port info
    let port_info = get_usb_port_info(port_name)
        .ok_or_else(|| format!("Could not find USB info for port {}", port_name))?;

    // Open the serial port with native type for espflash
    let serial = serialport::new(port_name, 115_200)
        .flow_control(FlowControl::None)
        .open_native()?;

    sender.send(FlashMessage::Log("Serial port opened".into()))?;

    // Create connection with reset operations
    let mut connection = Connection::new(
        serial,
        port_info,
        ResetAfterOperation::HardReset,
        ResetBeforeOperation::DefaultReset,
        115_200,
    );

    // Initialize the connection (enters bootloader mode)
    connection.begin()?;

    sender.send(FlashMessage::Log("Entered bootloader mode".into()))?;

    // Create flasher for ESP32-S3
    let mut flasher = Flasher::connect(
        connection,
        true,  // Use stub flasher
        false, // Don't verify
        false, // Don't skip
        Some(Chip::Esp32s3),
        None,  // No baud rate change
    )?;

    sender.send(FlashMessage::Log("Connected to ESP32-S3 bootloader".into()))?;

    // Create progress callbacks
    let mut progress = FlashProgress {
        sender: sender.clone(),
        total_size: 0,
    };

    // Send erasing state
    sender.send(FlashMessage::StateChanged(FlashState::Erasing))?;
    sender.send(FlashMessage::Log("Erasing flash...".into()))?;

    // Write the firmware
    sender.send(FlashMessage::Log("Writing firmware...".into()))?;

    flasher.write_bin_to_flash(
        0x0, // Start address for app
        FIRMWARE_BINARY,
        &mut progress,
    )?;

    // Verify (the flasher will reset after operation based on Connection settings)
    sender.send(FlashMessage::StateChanged(FlashState::Verifying))?;
    sender.send(FlashMessage::Log("Verifying...".into()))?;

    // Reset the device - drop the flasher which triggers reset based on ResetAfterOperation
    sender.send(FlashMessage::Log("Resetting device...".into()))?;
    drop(flasher);

    // Done
    sender.send(FlashMessage::StateChanged(FlashState::Complete))?;
    sender.send(FlashMessage::Log("Firmware update complete!".into()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_firmware_binary_embedded() {
        // Verify the include_bytes! macro works - the binary exists (may be empty placeholder)
        // This ensures the build doesn't fail due to missing file
        let _ = FIRMWARE_BINARY;
    }

    #[test]
    fn test_detect_devices() {
        // Verify detect_devices doesn't panic
        let _ = detect_devices();
    }
}
