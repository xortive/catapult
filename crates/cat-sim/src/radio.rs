//! Virtual radio simulation
//!
//! Provides a simulated radio that generates protocol-accurate output
//! when its state changes.

use std::collections::VecDeque;
use std::time::Instant;

use cat_protocol::{
    elecraft::ElecraftCommand, flex::FlexCommand, icom::CivCommand, kenwood::KenwoodCommand,
    yaesu::YaesuCommand, EncodeCommand, FromRadioCommand, OperatingMode, Protocol, RadioCommand,
    RadioDatabase, RadioModel,
};
use serde::{Deserialize, Serialize};

/// A simulated radio that generates protocol-accurate output
#[derive(Debug)]
pub struct VirtualRadio {
    /// Unique identifier for this virtual radio
    id: String,
    /// Protocol used for encoding commands
    protocol: Protocol,
    /// Radio model (for ID responses)
    model: Option<RadioModel>,
    /// Current frequency in Hz
    frequency_hz: u64,
    /// Current operating mode
    mode: OperatingMode,
    /// PTT active state
    ptt: bool,
    /// CI-V address (for Icom protocol)
    civ_address: Option<u8>,
    /// Auto-information mode enabled
    /// When true, radio sends unsolicited updates on state changes
    auto_info_enabled: bool,
    /// Pending output bytes (protocol-encoded)
    pending_output: VecDeque<Vec<u8>>,
    /// Last state change timestamp
    last_change: Instant,
}

/// Configuration for creating a virtual radio
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VirtualRadioConfig {
    /// Display name/identifier
    pub id: String,
    /// Protocol to use for output encoding
    pub protocol: Protocol,
    /// Radio model name (for ID responses)
    #[serde(default)]
    pub model_name: Option<String>,
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
            model_name: None,
            initial_frequency_hz: 14_250_000, // 20m
            initial_mode: OperatingMode::Usb,
            civ_address: None,
        }
    }
}

impl VirtualRadio {
    /// Create a new virtual radio with default settings
    pub fn new(id: impl Into<String>, protocol: Protocol) -> Self {
        let model = RadioDatabase::default_for_protocol(protocol);
        let civ_address = model.as_ref().and_then(|m| {
            if let cat_protocol::ProtocolId::CivAddress(addr) = &m.protocol_id {
                Some(*addr)
            } else {
                None
            }
        });
        Self {
            id: id.into(),
            protocol,
            model,
            frequency_hz: 14_250_000,
            mode: OperatingMode::Usb,
            ptt: false,
            civ_address,
            auto_info_enabled: false,
            pending_output: VecDeque::new(),
            last_change: Instant::now(),
        }
    }

    /// Create a virtual radio from configuration
    pub fn from_config(config: VirtualRadioConfig) -> Self {
        // Look up model by name, or use default for protocol
        let model = config
            .model_name
            .as_ref()
            .and_then(|name| {
                RadioDatabase::radios_for_protocol(config.protocol)
                    .into_iter()
                    .find(|m| m.model == *name)
            })
            .or_else(|| RadioDatabase::default_for_protocol(config.protocol));

        // Get CI-V address from model if not explicitly set
        let civ_address = config.civ_address.or_else(|| {
            model.as_ref().and_then(|m| {
                if let cat_protocol::ProtocolId::CivAddress(addr) = &m.protocol_id {
                    Some(*addr)
                } else {
                    None
                }
            })
        });

        Self {
            id: config.id,
            protocol: config.protocol,
            model,
            frequency_hz: config.initial_frequency_hz,
            mode: config.initial_mode,
            ptt: false,
            civ_address,
            auto_info_enabled: false,
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
        if self.protocol != protocol {
            self.protocol = protocol;
            // Update model to default for new protocol
            self.model = RadioDatabase::default_for_protocol(protocol);
            // Update CI-V address from model
            self.civ_address = self.model.as_ref().and_then(|m| {
                if let cat_protocol::ProtocolId::CivAddress(addr) = &m.protocol_id {
                    Some(*addr)
                } else {
                    None
                }
            });
        }
    }

    /// Get the radio model
    pub fn model(&self) -> Option<&RadioModel> {
        self.model.as_ref()
    }

    /// Get the model name for display
    pub fn model_name(&self) -> &str {
        self.model
            .as_ref()
            .map(|m| m.model.as_str())
            .unwrap_or("Unknown")
    }

    /// Set the radio model by name
    pub fn set_model(&mut self, model: Option<RadioModel>) {
        self.model = model;
        // Update CI-V address from model if using Icom protocol
        if self.protocol == Protocol::IcomCIV {
            self.civ_address = self.model.as_ref().and_then(|m| {
                if let cat_protocol::ProtocolId::CivAddress(addr) = &m.protocol_id {
                    Some(*addr)
                } else {
                    None
                }
            });
        }
    }

    /// Get the current frequency in Hz
    pub fn frequency_hz(&self) -> u64 {
        self.frequency_hz
    }

    /// Set the frequency and queue a protocol-encoded command if auto-info is enabled
    pub fn set_frequency(&mut self, hz: u64) {
        if self.frequency_hz != hz {
            self.frequency_hz = hz;
            self.last_change = Instant::now();
            if self.auto_info_enabled {
                self.queue_command(RadioCommand::FrequencyReport { hz });
            }
        }
    }

    /// Get the current operating mode
    pub fn mode(&self) -> OperatingMode {
        self.mode
    }

    /// Set the operating mode and queue a protocol-encoded command if auto-info is enabled
    pub fn set_mode(&mut self, mode: OperatingMode) {
        if self.mode != mode {
            self.mode = mode;
            self.last_change = Instant::now();
            if self.auto_info_enabled {
                self.queue_command(RadioCommand::ModeReport { mode });
            }
        }
    }

    /// Get the PTT state
    pub fn ptt(&self) -> bool {
        self.ptt
    }

    /// Set the PTT state and queue a protocol-encoded command if auto-info is enabled
    pub fn set_ptt(&mut self, active: bool) {
        if self.ptt != active {
            self.ptt = active;
            self.last_change = Instant::now();
            if self.auto_info_enabled {
                self.queue_command(RadioCommand::PttReport { active });
            }
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

    /// Get the auto-information mode state
    pub fn auto_info_enabled(&self) -> bool {
        self.auto_info_enabled
    }

    /// Set the auto-information mode state
    /// When enabled, the radio will send unsolicited updates on state changes
    pub fn set_auto_info(&mut self, enabled: bool) {
        if self.auto_info_enabled != enabled {
            self.auto_info_enabled = enabled;
            // Send confirmation response
            self.queue_command(RadioCommand::AutoInfoReport { enabled });
        }
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

    /// Send an ID response based on the current model
    pub fn send_id_response(&mut self) {
        let id = self.get_id_string();
        self.queue_command(RadioCommand::IdReport { id });
    }

    /// Get the ID string based on the current model and protocol
    pub fn get_id_string(&self) -> String {
        if let Some(model) = &self.model {
            match &model.protocol_id {
                cat_protocol::ProtocolId::KenwoodId(id) => id.clone(),
                cat_protocol::ProtocolId::ElecraftId(id) => id.clone(),
                cat_protocol::ProtocolId::FlexId(id) => id.clone(),
                cat_protocol::ProtocolId::CivAddress(addr) => format!("{:02X}", addr),
                cat_protocol::ProtocolId::YaesuCode(code) => format!("{:02X}", code),
            }
        } else {
            // Default IDs if no model set
            match self.protocol {
                Protocol::Kenwood => "023".to_string(),   // TS-590SG
                Protocol::Elecraft => "K3".to_string(),   // K3
                Protocol::FlexRadio => "909".to_string(), // FLEX-6600
                Protocol::IcomCIV => "94".to_string(),    // IC-7300
                Protocol::Yaesu => "01".to_string(),      // FT-991A
            }
        }
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
            Protocol::Kenwood => KenwoodCommand::from_radio_command(cmd).map(|c| c.encode()),
            Protocol::Elecraft => ElecraftCommand::from_radio_command(cmd).map(|c| c.encode()),
            Protocol::IcomCIV => {
                CivCommand::from_radio_command(cmd).map(|c| {
                    // For Icom, set proper addresses
                    let addr = self.civ_address.unwrap_or(0x94); // Default to IC-7300
                    CivCommand::new(0xE0, addr, c.command).encode()
                })
            }
            Protocol::Yaesu => YaesuCommand::from_radio_command(cmd).map(|c| c.encode()),
            Protocol::FlexRadio => FlexCommand::from_radio_command(cmd).map(|c| c.encode()),
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

    /// Handle an incoming RadioCommand and generate appropriate responses
    /// Returns true if the command was handled
    pub fn handle_command(&mut self, cmd: &RadioCommand) -> bool {
        match cmd {
            RadioCommand::SetFrequency { hz } => {
                self.frequency_hz = *hz;
                self.last_change = Instant::now();
                if self.auto_info_enabled {
                    self.queue_command(RadioCommand::FrequencyReport { hz: *hz });
                }
                true
            }
            RadioCommand::GetFrequency => {
                self.queue_command(RadioCommand::FrequencyReport {
                    hz: self.frequency_hz,
                });
                true
            }
            RadioCommand::SetMode { mode } => {
                self.mode = *mode;
                self.last_change = Instant::now();
                if self.auto_info_enabled {
                    self.queue_command(RadioCommand::ModeReport { mode: *mode });
                }
                true
            }
            RadioCommand::GetMode => {
                self.queue_command(RadioCommand::ModeReport { mode: self.mode });
                true
            }
            RadioCommand::SetPtt { active } => {
                self.ptt = *active;
                self.last_change = Instant::now();
                if self.auto_info_enabled {
                    self.queue_command(RadioCommand::PttReport { active: *active });
                }
                true
            }
            RadioCommand::GetPtt => {
                self.queue_command(RadioCommand::PttReport { active: self.ptt });
                true
            }
            RadioCommand::GetId => {
                self.send_id_response();
                true
            }
            RadioCommand::GetStatus => {
                self.send_status_report();
                true
            }
            RadioCommand::EnableAutoInfo { enabled } => {
                self.auto_info_enabled = *enabled;
                self.queue_command(RadioCommand::AutoInfoReport { enabled: *enabled });
                true
            }
            RadioCommand::GetAutoInfo => {
                self.queue_command(RadioCommand::AutoInfoReport {
                    enabled: self.auto_info_enabled,
                });
                true
            }
            _ => false,
        }
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
    fn test_set_frequency_no_output_without_auto_info() {
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);
        radio.set_frequency(7_074_000);

        // No output when auto_info is disabled
        assert!(!radio.has_output());
        assert_eq!(radio.frequency_hz(), 7_074_000);
    }

    #[test]
    fn test_set_frequency_generates_output_with_auto_info() {
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);
        radio.set_auto_info(true);
        radio.clear_output(); // Clear the AI response

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
    fn test_set_mode_generates_output_with_auto_info() {
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);
        radio.set_auto_info(true);
        radio.clear_output();

        radio.set_mode(OperatingMode::Cw);

        assert!(radio.has_output());
        let output = radio.take_output().unwrap();

        // Should be Kenwood format: MD3;
        let s = String::from_utf8_lossy(&output);
        assert!(s.contains("MD"));
        assert!(s.ends_with(";"));
    }

    #[test]
    fn test_set_ptt_generates_output_with_auto_info() {
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);
        radio.set_auto_info(true);
        radio.clear_output();

        radio.set_ptt(true);

        assert!(radio.has_output());
        let output = radio.take_output().unwrap();

        // Should be Kenwood format: TX1;
        assert_eq!(output, b"TX1;");
    }

    #[test]
    fn test_no_output_when_value_unchanged() {
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);
        radio.set_auto_info(true);
        radio.clear_output();

        // Set to same value as initial
        radio.set_frequency(14_250_000);
        assert!(!radio.has_output());

        radio.set_mode(OperatingMode::Usb);
        assert!(!radio.has_output());

        radio.set_ptt(false);
        assert!(!radio.has_output());
    }

    #[test]
    fn test_auto_info_enable_disable() {
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);
        assert!(!radio.auto_info_enabled());

        radio.set_auto_info(true);
        assert!(radio.auto_info_enabled());
        assert!(radio.has_output()); // Should send AI1;

        let output = radio.take_output().unwrap();
        let s = String::from_utf8_lossy(&output);
        assert!(s.contains("AI"));
        assert!(s.contains("1"));

        radio.set_auto_info(false);
        assert!(!radio.auto_info_enabled());
    }

    #[test]
    fn test_handle_enable_auto_info_command() {
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);

        let handled = radio.handle_command(&RadioCommand::EnableAutoInfo { enabled: true });
        assert!(handled);
        assert!(radio.auto_info_enabled());
        assert!(radio.has_output());
    }

    #[test]
    fn test_handle_get_frequency_command() {
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);

        let handled = radio.handle_command(&RadioCommand::GetFrequency);
        assert!(handled);
        assert!(radio.has_output());

        let output = radio.take_output().unwrap();
        let s = String::from_utf8_lossy(&output);
        assert!(s.contains("FA"));
    }

    #[test]
    fn test_icom_encoding_with_auto_info() {
        let mut radio = VirtualRadio::new("IC-7300", Protocol::IcomCIV);
        radio.set_civ_address(Some(0x94));
        radio.set_auto_info(true);
        radio.clear_output();

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
    fn test_frequency_command_encoding_with_auto_info() {
        // Test that frequency changes generate proper output when AI enabled
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);
        radio.set_auto_info(true);
        radio.clear_output();

        radio.set_frequency(21_350_000);
        radio.set_mode(OperatingMode::Cw);

        // Should have output for both frequency and mode changes
        assert!(radio.output_count() >= 2);
    }

    #[test]
    fn test_multiple_outputs_queued_with_auto_info() {
        let mut radio = VirtualRadio::new("Test", Protocol::Kenwood);
        radio.set_auto_info(true);
        radio.clear_output();

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
            model_name: Some("K3".to_string()),
            initial_frequency_hz: 10_125_000,
            initial_mode: OperatingMode::Cw,
            civ_address: None,
        };

        let radio = VirtualRadio::from_config(config);
        assert_eq!(radio.id(), "My Radio");
        assert_eq!(radio.protocol(), Protocol::Elecraft);
        assert_eq!(radio.frequency_hz(), 10_125_000);
        assert_eq!(radio.mode(), OperatingMode::Cw);
        assert_eq!(radio.model_name(), "K3");
    }
}
