//! Normalized radio command representation
//!
//! This module provides the `RadioCommand` enum which serves as the common
//! intermediate representation for commands across all CAT protocols.

/// Operating modes supported by amateur radio transceivers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum OperatingMode {
    /// Lower Sideband
    Lsb,
    /// Upper Sideband
    Usb,
    /// Continuous Wave
    Cw,
    /// CW Reverse
    CwR,
    /// Amplitude Modulation
    Am,
    /// Frequency Modulation
    Fm,
    /// FM Narrow
    FmN,
    /// Digital modes (RTTY, PSK, etc.)
    Dig,
    /// Digital Upper
    DigU,
    /// Digital Lower
    DigL,
    /// Packet
    Pkt,
    /// Data mode (generic)
    Data,
    /// Data Upper
    DataU,
    /// Data Lower
    DataL,
    /// RTTY
    Rtty,
    /// RTTY Reverse
    RttyR,
}

impl OperatingMode {
    /// Returns whether this is a voice mode
    pub fn is_voice(&self) -> bool {
        matches!(
            self,
            Self::Lsb | Self::Usb | Self::Am | Self::Fm | Self::FmN
        )
    }

    /// Returns whether this is a digital/data mode
    pub fn is_digital(&self) -> bool {
        matches!(
            self,
            Self::Dig
                | Self::DigU
                | Self::DigL
                | Self::Data
                | Self::DataU
                | Self::DataL
                | Self::Pkt
                | Self::Rtty
                | Self::RttyR
        )
    }

    /// Returns whether this is a CW mode
    pub fn is_cw(&self) -> bool {
        matches!(self, Self::Cw | Self::CwR)
    }
}

/// Normalized radio command that can be translated between protocols
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RadioCommand {
    /// Set the VFO frequency in Hz
    SetFrequency { hz: u64 },

    /// Get the current VFO frequency
    GetFrequency,

    /// Frequency report (response to GetFrequency)
    FrequencyReport { hz: u64 },

    /// Set the operating mode
    SetMode { mode: OperatingMode },

    /// Get the current operating mode
    GetMode,

    /// Mode report (response to GetMode)
    ModeReport { mode: OperatingMode },

    /// Set PTT state
    SetPtt { active: bool },

    /// Get PTT state
    GetPtt,

    /// PTT state report
    PttReport { active: bool },

    /// Set VFO (A, B, or split)
    SetVfo { vfo: Vfo },

    /// Get current VFO selection
    GetVfo,

    /// VFO selection report
    VfoReport { vfo: Vfo },

    /// Request radio identification
    GetId,

    /// Radio identification response
    IdReport { id: String },

    /// Request radio status (comprehensive)
    GetStatus,

    /// Radio status report
    StatusReport {
        frequency_hz: Option<u64>,
        mode: Option<OperatingMode>,
        ptt: Option<bool>,
        vfo: Option<Vfo>,
    },

    /// Power on/off command
    SetPower { on: bool },

    /// Enable/disable auto-information mode
    /// When enabled, radio sends unsolicited updates when parameters change
    EnableAutoInfo { enabled: bool },

    /// Query auto-information state
    GetAutoInfo,

    /// Auto-information state report
    AutoInfoReport { enabled: bool },

    /// Get control band (which VFO has front panel control)
    /// TS-990S specific: CB; query
    GetControlBand,

    /// Control band report (0=Main/A, 1=Sub/B)
    ControlBandReport { band: u8 },

    /// Get transmit band (which VFO is selected for transmit)
    /// Critical for split operation: TB; query
    GetTransmitBand,

    /// Transmit band report (0=Main/A, 1=Sub/B)
    TransmitBandReport { band: u8 },

    /// Unknown or unparseable command (preserves raw data)
    Unknown { data: Vec<u8> },
}

/// VFO selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Vfo {
    /// VFO A
    A,
    /// VFO B
    B,
    /// Split operation (TX on B, RX on A)
    Split,
    /// Memory channel
    Memory,
}

impl RadioCommand {
    /// Returns true if this is a query/request command
    pub fn is_query(&self) -> bool {
        matches!(
            self,
            Self::GetFrequency
                | Self::GetMode
                | Self::GetPtt
                | Self::GetVfo
                | Self::GetId
                | Self::GetStatus
                | Self::GetAutoInfo
                | Self::GetControlBand
                | Self::GetTransmitBand
        )
    }

    /// Returns true if this is a response/report command
    pub fn is_report(&self) -> bool {
        matches!(
            self,
            Self::FrequencyReport { .. }
                | Self::ModeReport { .. }
                | Self::PttReport { .. }
                | Self::VfoReport { .. }
                | Self::IdReport { .. }
                | Self::StatusReport { .. }
                | Self::AutoInfoReport { .. }
                | Self::ControlBandReport { .. }
                | Self::TransmitBandReport { .. }
        )
    }

    /// Returns true if this is a set/action command
    pub fn is_set(&self) -> bool {
        matches!(
            self,
            Self::SetFrequency { .. }
                | Self::SetMode { .. }
                | Self::SetPtt { .. }
                | Self::SetVfo { .. }
                | Self::SetPower { .. }
                | Self::EnableAutoInfo { .. }
        )
    }

    /// Extract frequency from command if present
    pub fn frequency(&self) -> Option<u64> {
        match self {
            Self::SetFrequency { hz } | Self::FrequencyReport { hz } => Some(*hz),
            Self::StatusReport { frequency_hz, .. } => *frequency_hz,
            _ => None,
        }
    }

    /// Extract mode from command if present
    pub fn mode(&self) -> Option<OperatingMode> {
        match self {
            Self::SetMode { mode } | Self::ModeReport { mode } => Some(*mode),
            Self::StatusReport { mode, .. } => *mode,
            _ => None,
        }
    }

    /// Extract PTT state from command if present
    pub fn ptt(&self) -> Option<bool> {
        match self {
            Self::SetPtt { active } | Self::PttReport { active } => Some(*active),
            Self::StatusReport { ptt, .. } => *ptt,
            _ => None,
        }
    }
}
