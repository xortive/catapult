//! Radio state tracking

use std::time::Instant;

use cat_protocol::{OperatingMode, Protocol, RadioModel};
use serde::{Deserialize, Serialize};

/// Unique identifier for a radio in the multiplexer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RadioHandle(pub u32);

impl RadioHandle {
    /// Get the raw handle value
    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

/// Current state of a connected radio
#[derive(Debug, Clone)]
pub struct RadioState {
    /// Unique handle
    pub handle: RadioHandle,
    /// Display name
    pub name: String,
    /// Serial port
    pub port: String,
    /// Protocol in use
    pub protocol: Protocol,
    /// Identified radio model
    pub model: Option<RadioModel>,
    /// Current frequency in Hz
    pub frequency_hz: Option<u64>,
    /// Current operating mode
    pub mode: Option<OperatingMode>,
    /// PTT active
    pub ptt: bool,
    /// CI-V address (for Icom)
    pub civ_address: Option<u8>,
    /// Last activity timestamp
    pub last_activity: Instant,
    /// Last frequency change timestamp
    pub last_freq_change: Option<Instant>,
    /// Whether this is a simulated radio
    pub is_simulated: bool,
}

impl RadioState {
    /// Create a new radio state
    pub fn new(handle: RadioHandle, name: String, port: String, protocol: Protocol) -> Self {
        Self {
            handle,
            name,
            port,
            protocol,
            model: None,
            frequency_hz: None,
            mode: None,
            ptt: false,
            civ_address: None,
            last_activity: Instant::now(),
            last_freq_change: None,
            is_simulated: false,
        }
    }

    /// Create a new simulated radio state
    pub fn new_simulated(handle: RadioHandle, name: String, protocol: Protocol) -> Self {
        Self {
            handle,
            name,
            port: "[SIM]".to_string(),
            protocol,
            model: None,
            frequency_hz: None,
            mode: None,
            ptt: false,
            civ_address: None,
            last_activity: Instant::now(),
            last_freq_change: None,
            is_simulated: true,
        }
    }

    /// Update activity timestamp
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Update frequency
    pub fn set_frequency(&mut self, hz: u64) {
        if self.frequency_hz != Some(hz) {
            self.frequency_hz = Some(hz);
            self.last_freq_change = Some(Instant::now());
        }
        self.touch();
    }

    /// Update mode
    pub fn set_mode(&mut self, mode: OperatingMode) {
        self.mode = Some(mode);
        self.touch();
    }

    /// Update PTT state
    pub fn set_ptt(&mut self, ptt: bool) {
        self.ptt = ptt;
        self.touch();
    }

    /// Format frequency for display
    pub fn frequency_display(&self) -> String {
        match self.frequency_hz {
            Some(hz) => {
                let mhz = hz as f64 / 1_000_000.0;
                format!("{:.3} MHz", mhz)
            }
            None => "---".to_string(),
        }
    }

    /// Format mode for display
    pub fn mode_display(&self) -> String {
        match self.mode {
            Some(mode) => format!("{:?}", mode),
            None => "---".to_string(),
        }
    }
}

/// Switching mode for the multiplexer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SwitchingMode {
    /// User manually selects the active radio
    Manual,
    /// Switch when a radio changes frequency
    #[default]
    FrequencyTriggered,
    /// Combination of PTT and frequency (legacy)
    Automatic,
}

impl SwitchingMode {
    /// Get human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::FrequencyTriggered => "Frequency Triggered",
            Self::Automatic => "Automatic",
        }
    }

    /// Get description
    pub fn description(&self) -> &'static str {
        match self {
            Self::Manual => "Manually select which radio controls the amplifier",
            Self::FrequencyTriggered => "Switch when a radio changes operating frequency",
            Self::Automatic => "Switch on PTT or frequency change",
        }
    }
}

/// Amplifier output configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmplifierConfig {
    /// Serial port for amplifier
    pub port: String,
    /// Protocol to use
    pub protocol: Protocol,
    /// Baud rate
    pub baud_rate: u32,
    /// CI-V address (if using Icom)
    pub civ_address: Option<u8>,
}

impl Default for AmplifierConfig {
    fn default() -> Self {
        Self {
            port: String::new(),
            protocol: Protocol::Kenwood,
            baud_rate: 38400,
            civ_address: None,
        }
    }
}

/// State the amplifier believes the radio is in
///
/// This tracks what the mux has told the amplifier, allowing us to:
/// - Respond to amplifier queries from cached state
/// - Send unsolicited updates when auto-info is enabled
#[derive(Debug, Clone, Default)]
pub struct AmplifierEmulatedState {
    /// Frequency in Hz that amp believes radio is on
    pub frequency_hz: Option<u64>,
    /// Operating mode that amp believes radio is in
    pub mode: Option<OperatingMode>,
    /// PTT state that amp believes radio is in
    pub ptt: bool,
    /// Whether auto-info mode is enabled (amp wants unsolicited updates)
    pub auto_info_enabled: bool,
}
