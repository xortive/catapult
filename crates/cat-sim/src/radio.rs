//! Virtual radio simulation
//!
//! Provides a simulated radio that generates protocol-accurate output
//! when its state changes.

use std::collections::VecDeque;
use std::time::Instant;

use cat_protocol::{
    elecraft::ElecraftCommand,
    flex::FlexCommand,
    icom::CivCommand,
    kenwood::KenwoodCommand,
    yaesu::YaesuCommand,
    EncodeCommand, FromRadioCommand, OperatingMode, Protocol, RadioCommand,
};
use serde::{Deserialize, Serialize};

/// A simulated radio that generates protocol-accurate output
#[derive(Debug)]
pub struct VirtualRadio {
    /// Unique identifier for this virtual radio
    id: String,
    /// Protocol used for encoding commands
    protocol: Protocol,
    /// Current frequency in Hz
    frequency_hz: u64,
    /// Current operating mode
    mode: OperatingMode,
    /// PTT active state
    ptt: bool,
    /// CI-V address (for Icom protocol)
    civ_address: Option<u8>,
    /// Pending output bytes (protocol-encoded)
    pending_output: VecDeque<Vec<u8>>,
    /// Last state change timestamp
    last_change: Instant,
}

/// Configuration for creating a virtual radio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualRadioConfig {
    /// Display name/identifier
    pub id: String,
    /// Protocol to use for output encoding
    pub protocol: Protocol,
    /// Initial frequency in Hz
    pub initial_frequency_hz: u64,
    /// Initial operating mode
    pub initial_mode: OperatingMode,
    /// CI-V address (for Icom protocol)
    pub civ_address: Option<u8>,
}

impl Default for VirtualRadioConfig {
    fn default() -> Self {
        Self {
            id: "Virtual Radio".to_string(),
            protocol: Protocol::Kenwood,
            initial_frequency_hz: 14_250_000, // 20m
            initial_mode: OperatingMode::Usb,
            civ_address: None,
        }
    }
}

impl VirtualRadio {
    /// Create a new virtual radio with default settings
    pub fn new(id: impl Into<String>, protocol: Protocol) -> Self {
        Self {
            id: id.into(),
            protocol,
            frequency_hz: 14_250_000,
            mode: OperatingMode::Usb,
            ptt: false,
            civ_address: None,
            pending_output: VecDeque::new(),
            last_change: Instant::now(),
        }
    }

    /// Create a virtual radio from configuration
    pub fn from_config(config: VirtualRadioConfig) -> Self {
        Self {
            id: config.id,
            protocol: config.protocol,
            frequency_hz: config.initial_frequency_hz,
            mode: config.initial_mode,
            ptt: false,
            civ_address: config.civ_address,
            pending_output: VecDeque::new(),
            last_change: Instant::now(),
        }
    }

    /// Get the radio's unique identifier
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get the protocol used by this radio
    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    /// Set the protocol (re-encodes future commands)
    pub fn set_protocol(&mut self, protocol: Protocol) {
        self.protocol = protocol;
    }

    /// Get the current frequency in Hz
    pub fn frequency_hz(&self) -> u64 {
        self.frequency_hz
    }

    /// Set the frequency and queue a protocol-encoded command
    pub fn set_frequency(&mut self, hz: u64) {
        if self.frequency_hz != hz {
            self.frequency_hz = hz;
            self.last_change = Instant::now();
            self.queue_command(RadioCommand::FrequencyReport { hz });
        }
    }

    /// Get the current operating mode
    pub fn mode(&self) -> OperatingMode {
        self.mode
    }

    /// Set the operating mode and queue a protocol-encoded command
    pub fn set_mode(&mut self, mode: OperatingMode) {
        if self.mode != mode {
            self.mode = mode;
            self.last_change = Instant::now();
            self.queue_command(RadioCommand::ModeReport { mode });
        }
    }

    /// Get the PTT state
    pub fn ptt(&self) -> bool {
        self.ptt
    }

    /// Set the PTT state and queue a protocol-encoded command
    pub fn set_ptt(&mut self, active: bool) {
        if self.ptt != active {
            self.ptt = active;
            self.last_change = Instant::now();
            self.queue_command(RadioCommand::PttReport { active });
        }
    }

    /// Get the CI-V address (Icom only)
    pub fn civ_address(&self) -> Option<u8> {
        self.civ_address
    }

    /// Set the CI-V address (Icom only)
    pub fn set_civ_address(&mut self, addr: Option<u8>) {
        self.civ_address = addr;
    }

    /// Get the time of last state change
    pub fn last_change(&self) -> Instant {
        self.last_change
    }

    /// Queue a RadioCommand, encoding it to the appropriate protocol
    pub fn queue_command(&mut self, cmd: RadioCommand) {
        if let Some(encoded) = self.encode_command(&cmd) {
            self.pending_output.push_back(encoded);
        }
    }

    /// Send a full status report
    pub fn send_status_report(&mut self) {
        let cmd = RadioCommand::StatusReport {
            frequency_hz: Some(self.frequency_hz),
            mode: Some(self.mode),
            ptt: Some(self.ptt),
            vfo: None,
        };
        self.queue_command(cmd);
    }

    /// Take the next pending output bytes
    pub fn take_output(&mut self) -> Option<Vec<u8>> {
        self.pending_output.pop_front()
    }

    /// Check if there is pending output
    pub fn has_output(&self) -> bool {
        !self.pending_output.is_empty()
    }

    /// Clear all pending output
    pub fn clear_output(&mut self) {
        self.pending_output.clear();
    }

    /// Get the number of pending output messages
    pub fn output_count(&self) -> usize {
        self.pending_output.len()
    }

    /// Encode a RadioCommand to protocol bytes
    fn encode_command(&self, cmd: &RadioCommand) -> Option<Vec<u8>> {
        match self.protocol {
            Protocol::Kenwood => {
                KenwoodCommand::from_radio_command(cmd).map(|c| c.encode())
            }
            Protocol::Elecraft => {
                ElecraftCommand::from_radio_command(cmd).map(|c| c.encode())
            }
            Protocol::IcomCIV => {
                CivCommand::from_radio_command(cmd).map(|c| {
                    // For Icom, set proper addresses
                    let addr = self.civ_address.unwrap_or(0x94); // Default to IC-7300
                    CivCommand::new(0xE0, addr, c.command).encode()
                })
            }
            Protocol::Yaesu => {
                YaesuCommand::from_radio_command(cmd).map(|c| c.encode())
            }
            Protocol::FlexRadio => {
                FlexCommand::from_radio_command(cmd).map(|c| c.encode())
            }
        }
    }

    /// Format frequency for display
    pub fn frequency_display(&self) -> String {
        let mhz = self.frequency_hz as f64 / 1_000_000.0;
        format!("{:.3} MHz", mhz)
    }

    /// Format mode for display
    pub fn mode_display(&self) -> String {
        format!("{:?}", self.mode)
    }

    /// Get a summary of current state
    pub fn state_summary(&self) -> String {
        format!(
            "{} ({}) - {} {} {}",
            self.id,
            self.protocol.name(),
            self.frequency_display(),
            self.mode_display(),
            if self.ptt { "[TX]" } else { "" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_virtual_radio() {
        let radio = VirtualRadio::new("Test Radio", Protocol::Kenwood);
        assert_eq!(radio.id(), "Test Radio");
        assert_eq!(radio.protocol(), Protocol::Kenwood);
        assert_eq!(radio.frequency_hz(), 14_250_000);
        assert_eq!(radio.mode(), OperatingMode::Usb);
        assert!(!radio.ptt());
    }

    #[test]
    fn test_set_frequency_generates_output() {
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);
        radio.set_frequency(7_074_000);

        assert!(radio.has_output());
        let output = radio.take_output().unwrap();

        // Should be Kenwood format: FA00007074000;
        let s = String::from_utf8_lossy(&output);
        assert!(s.contains("FA"));
        assert!(s.contains("7074000"));
        assert!(s.ends_with(";"));
    }

    #[test]
    fn test_set_mode_generates_output() {
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);
        radio.set_mode(OperatingMode::Cw);

        assert!(radio.has_output());
        let output = radio.take_output().unwrap();

        // Should be Kenwood format: MD3;
        let s = String::from_utf8_lossy(&output);
        assert!(s.contains("MD"));
        assert!(s.ends_with(";"));
    }

    #[test]
    fn test_set_ptt_generates_output() {
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);
        radio.set_ptt(true);

        assert!(radio.has_output());
        let output = radio.take_output().unwrap();

        // Should be Kenwood format: TX1;
        assert_eq!(output, b"TX1;");
    }

    #[test]
    fn test_no_output_when_value_unchanged() {
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);

        // Set to same value as initial
        radio.set_frequency(14_250_000);
        assert!(!radio.has_output());

        radio.set_mode(OperatingMode::Usb);
        assert!(!radio.has_output());

        radio.set_ptt(false);
        assert!(!radio.has_output());
    }

    #[test]
    fn test_icom_encoding() {
        let mut radio = VirtualRadio::new("IC-7300", Protocol::IcomCIV);
        radio.set_civ_address(Some(0x94));
        // Use different frequency than default (14_250_000) to trigger change
        radio.set_frequency(7_074_000);

        let output = radio.take_output().unwrap();

        // CI-V frame should start with FE FE
        assert_eq!(output[0], 0xFE);
        assert_eq!(output[1], 0xFE);
        // Should end with FD
        assert_eq!(output[output.len() - 1], 0xFD);
    }

    #[test]
    fn test_frequency_command_encoding() {
        // Test that frequency changes generate proper output
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);
        radio.set_frequency(21_350_000);
        radio.set_mode(OperatingMode::Cw);

        // Should have output for both frequency and mode changes
        assert!(radio.output_count() >= 2);
    }

    #[test]
    fn test_multiple_outputs_queued() {
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);
        radio.set_frequency(7_074_000);
        radio.set_mode(OperatingMode::Dig);
        radio.set_ptt(true);

        assert_eq!(radio.output_count(), 3);

        radio.take_output();
        assert_eq!(radio.output_count(), 2);

        radio.clear_output();
        assert_eq!(radio.output_count(), 0);
    }

    #[test]
    fn test_from_config() {
        let config = VirtualRadioConfig {
            id: "My Radio".to_string(),
            protocol: Protocol::Elecraft,
            initial_frequency_hz: 10_125_000,
            initial_mode: OperatingMode::Cw,
            civ_address: None,
        };

        let radio = VirtualRadio::from_config(config);
        assert_eq!(radio.id(), "My Radio");
        assert_eq!(radio.protocol(), Protocol::Elecraft);
        assert_eq!(radio.frequency_hz(), 10_125_000);
        assert_eq!(radio.mode(), OperatingMode::Cw);
    }
}
