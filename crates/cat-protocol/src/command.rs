//! Normalized radio command representation
//!
//! This module provides `RadioRequest` and `RadioResponse` enums as the common
//! intermediate representation for commands across all CAT protocols.
//!
//! - `RadioRequest`: Commands/queries sent TO a radio (from mux or amplifier)
//! - `RadioResponse`: Reports/responses FROM a radio (to mux or amplifier)

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

/// Commands/queries sent TO a radio (from mux or amplifier)
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RadioRequest {
    /// Set the VFO frequency in Hz
    SetFrequency { hz: u64 },

    /// Set the operating mode
    SetMode { mode: OperatingMode },

    /// Set PTT state
    SetPtt { active: bool },

    /// Set VFO (A, B, or split)
    SetVfo { vfo: Vfo },

    /// Power on/off command
    SetPower { on: bool },

    /// Enable/disable auto-information mode
    SetAutoInfo { enabled: bool },

    /// Get the current VFO frequency
    GetFrequency,

    /// Get the current operating mode
    GetMode,

    /// Get PTT state
    GetPtt,

    /// Get current VFO selection
    GetVfo,

    /// Request radio identification
    GetId,

    /// Request radio status (comprehensive)
    GetStatus,

    /// Query auto-information state
    GetAutoInfo,

    /// Get control band (which VFO has front panel control)
    GetControlBand,

    /// Get transmit band (which VFO is selected for transmit)
    GetTransmitBand,

    /// Unknown or unparseable request (preserves raw data)
    Unknown { data: Vec<u8> },
}

/// Reports/responses FROM a radio (to mux or amplifier)
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RadioResponse {
    /// Frequency report
    Frequency { hz: u64 },

    /// Mode report
    Mode { mode: OperatingMode },

    /// PTT state report
    Ptt { active: bool },

    /// VFO selection report
    Vfo { vfo: Vfo },

    /// Radio identification response
    Id { id: String },

    /// Radio status report (comprehensive)
    Status {
        frequency_hz: Option<u64>,
        mode: Option<OperatingMode>,
        ptt: Option<bool>,
        vfo: Option<Vfo>,
    },

    /// Auto-information state report
    AutoInfo { enabled: bool },

    /// Control band report (0=Main/A, 1=Sub/B)
    ControlBand { band: u8 },

    /// Transmit band report (0=Main/A, 1=Sub/B)
    TransmitBand { band: u8 },

    /// Unknown or unparseable response (preserves raw data)
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

impl RadioRequest {
    /// Returns true if this is a query command (Get*)
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

    /// Returns true if this is a set/action command
    pub fn is_set(&self) -> bool {
        matches!(
            self,
            Self::SetFrequency { .. }
                | Self::SetMode { .. }
                | Self::SetPtt { .. }
                | Self::SetVfo { .. }
                | Self::SetPower { .. }
                | Self::SetAutoInfo { .. }
        )
    }

    /// Extract frequency from request if present
    pub fn frequency(&self) -> Option<u64> {
        match self {
            Self::SetFrequency { hz } => Some(*hz),
            _ => None,
        }
    }

    /// Extract mode from request if present
    pub fn mode(&self) -> Option<OperatingMode> {
        match self {
            Self::SetMode { mode } => Some(*mode),
            _ => None,
        }
    }

    /// Extract PTT state from request if present
    pub fn ptt(&self) -> Option<bool> {
        match self {
            Self::SetPtt { active } => Some(*active),
            _ => None,
        }
    }
}

impl RadioResponse {
    /// Extract frequency from response if present
    pub fn frequency(&self) -> Option<u64> {
        match self {
            Self::Frequency { hz } => Some(*hz),
            Self::Status { frequency_hz, .. } => *frequency_hz,
            _ => None,
        }
    }

    /// Extract mode from response if present
    pub fn mode(&self) -> Option<OperatingMode> {
        match self {
            Self::Mode { mode } => Some(*mode),
            Self::Status { mode, .. } => *mode,
            _ => None,
        }
    }

    /// Extract PTT state from response if present
    pub fn ptt(&self) -> Option<bool> {
        match self {
            Self::Ptt { active } => Some(*active),
            Self::Status { ptt, .. } => *ptt,
            _ => None,
        }
    }

    /// Extract VFO from response if present
    pub fn vfo(&self) -> Option<Vfo> {
        match self {
            Self::Vfo { vfo } => Some(*vfo),
            Self::Status { vfo, .. } => *vfo,
            _ => None,
        }
    }
}
