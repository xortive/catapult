//! Radio model database
//!
//! This module contains information about specific radio models,
//! their capabilities, and protocol-specific details.

use crate::{OperatingMode, Protocol};

/// Capabilities of a specific radio model (static version for database)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioCapabilitiesStatic {
    /// Supported operating modes (as a slice)
    pub modes: &'static [OperatingMode],
    /// Minimum frequency in Hz
    pub min_frequency_hz: u64,
    /// Maximum frequency in Hz
    pub max_frequency_hz: u64,
    /// Frequency resolution in Hz
    pub frequency_step_hz: u64,
    /// Supports split operation
    pub has_split: bool,
    /// Number of VFOs
    pub vfo_count: u8,
    /// Has built-in antenna tuner
    pub has_tuner: bool,
    /// Maximum TX power in watts
    pub max_power_watts: Option<u16>,
}

/// Capabilities of a specific radio model (owned version)
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RadioCapabilities {
    /// Supported operating modes
    pub modes: Vec<OperatingMode>,
    /// Minimum frequency in Hz
    pub min_frequency_hz: u64,
    /// Maximum frequency in Hz
    pub max_frequency_hz: u64,
    /// Frequency resolution in Hz
    pub frequency_step_hz: u64,
    /// Supports split operation
    pub has_split: bool,
    /// Number of VFOs
    pub vfo_count: u8,
    /// Has built-in antenna tuner
    pub has_tuner: bool,
    /// Maximum TX power in watts
    pub max_power_watts: Option<u16>,
}

impl From<RadioCapabilitiesStatic> for RadioCapabilities {
    fn from(s: RadioCapabilitiesStatic) -> Self {
        Self {
            modes: s.modes.to_vec(),
            min_frequency_hz: s.min_frequency_hz,
            max_frequency_hz: s.max_frequency_hz,
            frequency_step_hz: s.frequency_step_hz,
            has_split: s.has_split,
            vfo_count: s.vfo_count,
            has_tuner: s.has_tuner,
            max_power_watts: s.max_power_watts,
        }
    }
}

impl Default for RadioCapabilities {
    fn default() -> Self {
        Self {
            modes: vec![
                OperatingMode::Lsb,
                OperatingMode::Usb,
                OperatingMode::Cw,
                OperatingMode::Am,
                OperatingMode::Fm,
            ],
            min_frequency_hz: 100_000,
            max_frequency_hz: 60_000_000,
            frequency_step_hz: 10,
            has_split: true,
            vfo_count: 2,
            has_tuner: false,
            max_power_watts: Some(100),
        }
    }
}

/// Information about a specific radio model (static version)
#[derive(Debug, Clone, Copy)]
pub struct RadioModelStatic {
    /// Manufacturer name
    pub manufacturer: &'static str,
    /// Model name/number
    pub model: &'static str,
    /// Protocol used by this radio
    pub protocol: Protocol,
    /// Protocol-specific identifier
    pub protocol_id: ProtocolIdStatic,
    /// Radio capabilities
    pub capabilities: RadioCapabilitiesStatic,
}

/// Information about a specific radio model (owned version)
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RadioModel {
    /// Manufacturer name
    pub manufacturer: String,
    /// Model name/number
    pub model: String,
    /// Protocol used by this radio
    pub protocol: Protocol,
    /// Protocol-specific identifier
    pub protocol_id: ProtocolId,
    /// Radio capabilities
    pub capabilities: RadioCapabilities,
}

impl From<&RadioModelStatic> for RadioModel {
    fn from(s: &RadioModelStatic) -> Self {
        Self {
            manufacturer: s.manufacturer.to_string(),
            model: s.model.to_string(),
            protocol: s.protocol,
            protocol_id: s.protocol_id.into(),
            capabilities: s.capabilities.into(),
        }
    }
}

/// Protocol-specific radio identifier (static version)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolIdStatic {
    /// Icom CI-V address (0x00-0xFF)
    CivAddress(u8),
    /// Kenwood ID response code (e.g., "021" for TS-990S)
    KenwoodId(&'static str),
    /// Yaesu model code
    YaesuCode(u8),
    /// Elecraft model identifier
    ElecraftId(&'static str),
    /// FlexRadio ID response code (e.g., "905" for FLEX-6500)
    FlexId(&'static str),
}

/// Protocol-specific radio identifier (owned version)
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ProtocolId {
    /// Icom CI-V address (0x00-0xFF)
    CivAddress(u8),
    /// Kenwood ID response code (e.g., "021" for TS-990S)
    KenwoodId(String),
    /// Yaesu model code
    YaesuCode(u8),
    /// Elecraft model identifier
    ElecraftId(String),
    /// FlexRadio ID response code (e.g., "905" for FLEX-6500)
    FlexId(String),
}

impl From<ProtocolIdStatic> for ProtocolId {
    fn from(s: ProtocolIdStatic) -> Self {
        match s {
            ProtocolIdStatic::CivAddress(a) => Self::CivAddress(a),
            ProtocolIdStatic::KenwoodId(s) => Self::KenwoodId(s.to_string()),
            ProtocolIdStatic::YaesuCode(c) => Self::YaesuCode(c),
            ProtocolIdStatic::ElecraftId(s) => Self::ElecraftId(s.to_string()),
            ProtocolIdStatic::FlexId(s) => Self::FlexId(s.to_string()),
        }
    }
}

/// Database of known radio models
pub struct RadioDatabase;

impl RadioDatabase {
    /// Look up a radio model by CI-V address
    pub fn by_civ_address(address: u8) -> Option<RadioModel> {
        ICOM_RADIOS
            .iter()
            .find(|(addr, _)| *addr == address)
            .map(|(_, model)| model.into())
    }

    /// Look up a radio model by Kenwood ID
    pub fn by_kenwood_id(id: &str) -> Option<RadioModel> {
        KENWOOD_RADIOS
            .iter()
            .find(|(kid, _)| *kid == id)
            .map(|(_, model)| model.into())
    }

    /// Look up a radio model by Elecraft ID
    pub fn by_elecraft_id(id: &str) -> Option<RadioModel> {
        ELECRAFT_RADIOS
            .iter()
            .find(|(eid, _)| *eid == id)
            .map(|(_, model)| model.into())
    }

    /// Get all known Icom radios
    pub fn icom_radios() -> impl Iterator<Item = RadioModel> {
        ICOM_RADIOS.iter().map(|(_, model)| model.into())
    }

    /// Get all known Kenwood radios
    pub fn kenwood_radios() -> impl Iterator<Item = RadioModel> {
        KENWOOD_RADIOS.iter().map(|(_, model)| model.into())
    }

    /// Get all known Elecraft radios
    pub fn elecraft_radios() -> impl Iterator<Item = RadioModel> {
        ELECRAFT_RADIOS.iter().map(|(_, model)| model.into())
    }

    /// Look up a radio model by FlexRadio ID
    pub fn by_flex_id(id: &str) -> Option<RadioModel> {
        FLEX_RADIOS
            .iter()
            .find(|(fid, _)| *fid == id)
            .map(|(_, model)| model.into())
    }

    /// Get all known FlexRadio radios
    pub fn flex_radios() -> impl Iterator<Item = RadioModel> {
        FLEX_RADIOS.iter().map(|(_, model)| model.into())
    }

    /// Get all known Yaesu radios
    pub fn yaesu_radios() -> impl Iterator<Item = RadioModel> {
        YAESU_RADIOS.iter().map(|(_, model)| model.into())
    }

    /// Get all radios for a given protocol
    pub fn radios_for_protocol(protocol: Protocol) -> Vec<RadioModel> {
        match protocol {
            Protocol::IcomCIV => Self::icom_radios().collect(),
            Protocol::Kenwood => Self::kenwood_radios().collect(),
            Protocol::Elecraft => Self::elecraft_radios().collect(),
            Protocol::Yaesu => Self::yaesu_radios().collect(),
            Protocol::FlexRadio => Self::flex_radios().collect(),
        }
    }

    /// Get the default (most popular) radio model for a protocol
    pub fn default_for_protocol(protocol: Protocol) -> Option<RadioModel> {
        match protocol {
            Protocol::IcomCIV => Self::by_civ_address(0x94), // IC-7300
            Protocol::Kenwood => Self::by_kenwood_id("023"), // TS-590SG
            Protocol::Elecraft => Self::by_elecraft_id("K3"), // K3
            Protocol::Yaesu => YAESU_RADIOS.first().map(|(_, m)| m.into()),
            Protocol::FlexRadio => Self::by_flex_id("909"), // FLEX-6600
        }
    }
}

// Standard mode sets
static MODES_FULL_HF: &[OperatingMode] = &[
    OperatingMode::Lsb,
    OperatingMode::Usb,
    OperatingMode::Cw,
    OperatingMode::CwR,
    OperatingMode::Am,
    OperatingMode::Fm,
    OperatingMode::Rtty,
    OperatingMode::RttyR,
    OperatingMode::DataU,
    OperatingMode::DataL,
];

static MODES_STANDARD: &[OperatingMode] = &[
    OperatingMode::Lsb,
    OperatingMode::Usb,
    OperatingMode::Cw,
    OperatingMode::CwR,
    OperatingMode::Am,
    OperatingMode::Fm,
    OperatingMode::DataU,
    OperatingMode::DataL,
];

static MODES_BASIC: &[OperatingMode] = &[
    OperatingMode::Lsb,
    OperatingMode::Usb,
    OperatingMode::Cw,
    OperatingMode::Am,
    OperatingMode::Fm,
];

static MODES_NO_FM: &[OperatingMode] = &[
    OperatingMode::Lsb,
    OperatingMode::Usb,
    OperatingMode::Cw,
    OperatingMode::CwR,
    OperatingMode::DataU,
    OperatingMode::DataL,
];

// Icom CI-V address database
static ICOM_RADIOS: &[(u8, RadioModelStatic)] = &[
    (
        0x94,
        RadioModelStatic {
            manufacturer: "Icom",
            model: "IC-7300",
            protocol: Protocol::IcomCIV,
            protocol_id: ProtocolIdStatic::CivAddress(0x94),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FULL_HF,
                min_frequency_hz: 30_000,
                max_frequency_hz: 74_800_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,
                has_tuner: true,
                max_power_watts: Some(100),
            },
        },
    ),
    (
        0xA4,
        RadioModelStatic {
            manufacturer: "Icom",
            model: "IC-705",
            protocol: Protocol::IcomCIV,
            protocol_id: ProtocolIdStatic::CivAddress(0xA4),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_STANDARD,
                min_frequency_hz: 30_000,
                max_frequency_hz: 450_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,
                has_tuner: true,
                max_power_watts: Some(10),
            },
        },
    ),
    (
        0x98,
        RadioModelStatic {
            manufacturer: "Icom",
            model: "IC-7610",
            protocol: Protocol::IcomCIV,
            protocol_id: ProtocolIdStatic::CivAddress(0x98),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FULL_HF,
                min_frequency_hz: 30_000,
                max_frequency_hz: 60_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,
                has_tuner: true,
                max_power_watts: Some(100),
            },
        },
    ),
    (
        0x70,
        RadioModelStatic {
            manufacturer: "Icom",
            model: "IC-7000",
            protocol: Protocol::IcomCIV,
            protocol_id: ProtocolIdStatic::CivAddress(0x70),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_BASIC,
                min_frequency_hz: 30_000,
                max_frequency_hz: 450_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,
                has_tuner: false,
                max_power_watts: Some(100),
            },
        },
    ),
];

// Kenwood ID database
static KENWOOD_RADIOS: &[(&str, RadioModelStatic)] = &[
    (
        "021",
        RadioModelStatic {
            manufacturer: "Kenwood",
            model: "TS-990S",
            protocol: Protocol::Kenwood,
            protocol_id: ProtocolIdStatic::KenwoodId("021"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_STANDARD,
                min_frequency_hz: 30_000,
                max_frequency_hz: 60_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,
                has_tuner: true,
                max_power_watts: Some(200),
            },
        },
    ),
    (
        "023",
        RadioModelStatic {
            manufacturer: "Kenwood",
            model: "TS-590SG",
            protocol: Protocol::Kenwood,
            protocol_id: ProtocolIdStatic::KenwoodId("023"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_STANDARD,
                min_frequency_hz: 30_000,
                max_frequency_hz: 60_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,
                has_tuner: true,
                max_power_watts: Some(100),
            },
        },
    ),
    (
        "019",
        RadioModelStatic {
            manufacturer: "Kenwood",
            model: "TS-2000",
            protocol: Protocol::Kenwood,
            protocol_id: ProtocolIdStatic::KenwoodId("019"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_BASIC,
                min_frequency_hz: 30_000,
                max_frequency_hz: 1_300_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,
                has_tuner: true,
                max_power_watts: Some(100),
            },
        },
    ),
];

// Elecraft radio database
static ELECRAFT_RADIOS: &[(&str, RadioModelStatic)] = &[
    (
        "K3",
        RadioModelStatic {
            manufacturer: "Elecraft",
            model: "K3",
            protocol: Protocol::Elecraft,
            protocol_id: ProtocolIdStatic::ElecraftId("K3"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_STANDARD,
                min_frequency_hz: 500_000,
                max_frequency_hz: 54_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,
                has_tuner: true,
                max_power_watts: Some(100),
            },
        },
    ),
    (
        "K3S",
        RadioModelStatic {
            manufacturer: "Elecraft",
            model: "K3S",
            protocol: Protocol::Elecraft,
            protocol_id: ProtocolIdStatic::ElecraftId("K3S"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_STANDARD,
                min_frequency_hz: 500_000,
                max_frequency_hz: 54_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,
                has_tuner: true,
                max_power_watts: Some(100),
            },
        },
    ),
    (
        "KX3",
        RadioModelStatic {
            manufacturer: "Elecraft",
            model: "KX3",
            protocol: Protocol::Elecraft,
            protocol_id: ProtocolIdStatic::ElecraftId("KX3"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_STANDARD,
                min_frequency_hz: 500_000,
                max_frequency_hz: 54_000_000,
                frequency_step_hz: 10,
                has_split: true,
                vfo_count: 2,
                has_tuner: true,
                max_power_watts: Some(15),
            },
        },
    ),
    (
        "KX2",
        RadioModelStatic {
            manufacturer: "Elecraft",
            model: "KX2",
            protocol: Protocol::Elecraft,
            protocol_id: ProtocolIdStatic::ElecraftId("KX2"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_NO_FM,
                min_frequency_hz: 500_000,
                max_frequency_hz: 54_000_000,
                frequency_step_hz: 10,
                has_split: true,
                vfo_count: 2,
                has_tuner: true,
                max_power_watts: Some(12),
            },
        },
    ),
];

// FlexRadio SDR modes (includes all standard modes plus digital)
static MODES_FLEX_SDR: &[OperatingMode] = &[
    OperatingMode::Lsb,
    OperatingMode::Usb,
    OperatingMode::Cw,
    OperatingMode::CwR,
    OperatingMode::Am,
    OperatingMode::Fm,
    OperatingMode::FmN,
    OperatingMode::DigU,
    OperatingMode::DigL,
    OperatingMode::Rtty,
    OperatingMode::DataU,
    OperatingMode::DataL,
];

// FlexRadio ID database
// ID codes: 904=6700, 905=6500, 906=6700R, 907=6300, 908=6400, 909=6600, 910=6400M, 911=6600M, 912=8400, 913=8600
static FLEX_RADIOS: &[(&str, RadioModelStatic)] = &[
    // First Generation Signature Series (FLEX-6000)
    (
        "904",
        RadioModelStatic {
            manufacturer: "FlexRadio",
            model: "FLEX-6700",
            protocol: Protocol::FlexRadio,
            protocol_id: ProtocolIdStatic::FlexId("904"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FLEX_SDR,
                min_frequency_hz: 30_000,
                max_frequency_hz: 77_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 8, // Up to 8 slice receivers
                has_tuner: true,
                max_power_watts: Some(100),
            },
        },
    ),
    (
        "905",
        RadioModelStatic {
            manufacturer: "FlexRadio",
            model: "FLEX-6500",
            protocol: Protocol::FlexRadio,
            protocol_id: ProtocolIdStatic::FlexId("905"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FLEX_SDR,
                min_frequency_hz: 30_000,
                max_frequency_hz: 77_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 4, // Up to 4 slice receivers
                has_tuner: true,
                max_power_watts: Some(100),
            },
        },
    ),
    (
        "906",
        RadioModelStatic {
            manufacturer: "FlexRadio",
            model: "FLEX-6700R",
            protocol: Protocol::FlexRadio,
            protocol_id: ProtocolIdStatic::FlexId("906"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FLEX_SDR,
                min_frequency_hz: 30_000,
                max_frequency_hz: 77_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 8,          // Up to 8 slice receivers
                has_tuner: false,      // Receiver only
                max_power_watts: None, // Receiver only
            },
        },
    ),
    (
        "907",
        RadioModelStatic {
            manufacturer: "FlexRadio",
            model: "FLEX-6300",
            protocol: Protocol::FlexRadio,
            protocol_id: ProtocolIdStatic::FlexId("907"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FLEX_SDR,
                min_frequency_hz: 30_000,
                max_frequency_hz: 77_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,     // Up to 2 slice receivers
                has_tuner: false, // Optional ATU
                max_power_watts: Some(100),
            },
        },
    ),
    // Second Generation Signature Series (FLEX-6400/6600)
    (
        "908",
        RadioModelStatic {
            manufacturer: "FlexRadio",
            model: "FLEX-6400",
            protocol: Protocol::FlexRadio,
            protocol_id: ProtocolIdStatic::FlexId("908"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FLEX_SDR,
                min_frequency_hz: 30_000,
                max_frequency_hz: 77_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,     // Up to 2 slice receivers
                has_tuner: false, // Optional ATU
                max_power_watts: Some(100),
            },
        },
    ),
    (
        "909",
        RadioModelStatic {
            manufacturer: "FlexRadio",
            model: "FLEX-6600",
            protocol: Protocol::FlexRadio,
            protocol_id: ProtocolIdStatic::FlexId("909"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FLEX_SDR,
                min_frequency_hz: 30_000,
                max_frequency_hz: 77_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 4,     // Up to 4 slice receivers
                has_tuner: false, // Optional ATU
                max_power_watts: Some(100),
            },
        },
    ),
    (
        "910",
        RadioModelStatic {
            manufacturer: "FlexRadio",
            model: "FLEX-6400M",
            protocol: Protocol::FlexRadio,
            protocol_id: ProtocolIdStatic::FlexId("910"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FLEX_SDR,
                min_frequency_hz: 30_000,
                max_frequency_hz: 77_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,
                has_tuner: false,
                max_power_watts: Some(100),
            },
        },
    ),
    (
        "911",
        RadioModelStatic {
            manufacturer: "FlexRadio",
            model: "FLEX-6600M",
            protocol: Protocol::FlexRadio,
            protocol_id: ProtocolIdStatic::FlexId("911"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FLEX_SDR,
                min_frequency_hz: 30_000,
                max_frequency_hz: 77_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 4,
                has_tuner: false,
                max_power_watts: Some(100),
            },
        },
    ),
    // Third Generation Signature Series (FLEX-8000)
    (
        "912",
        RadioModelStatic {
            manufacturer: "FlexRadio",
            model: "FLEX-8400",
            protocol: Protocol::FlexRadio,
            protocol_id: ProtocolIdStatic::FlexId("912"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FLEX_SDR,
                min_frequency_hz: 30_000,
                max_frequency_hz: 77_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2, // Up to 2 slice receivers
                has_tuner: false,
                max_power_watts: Some(100),
            },
        },
    ),
    (
        "913",
        RadioModelStatic {
            manufacturer: "FlexRadio",
            model: "FLEX-8600",
            protocol: Protocol::FlexRadio,
            protocol_id: ProtocolIdStatic::FlexId("913"),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FLEX_SDR,
                min_frequency_hz: 30_000,
                max_frequency_hz: 77_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 4, // Up to 4 slice receivers
                has_tuner: false,
                max_power_watts: Some(100),
            },
        },
    ),
];

// Yaesu radio database (keyed by model code)
static YAESU_RADIOS: &[(u8, RadioModelStatic)] = &[
    (
        0x01,
        RadioModelStatic {
            manufacturer: "Yaesu",
            model: "FT-991A",
            protocol: Protocol::Yaesu,
            protocol_id: ProtocolIdStatic::YaesuCode(0x01),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FULL_HF,
                min_frequency_hz: 30_000,
                max_frequency_hz: 450_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,
                has_tuner: true,
                max_power_watts: Some(100),
            },
        },
    ),
    (
        0x02,
        RadioModelStatic {
            manufacturer: "Yaesu",
            model: "FTDX101D",
            protocol: Protocol::Yaesu,
            protocol_id: ProtocolIdStatic::YaesuCode(0x02),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FULL_HF,
                min_frequency_hz: 30_000,
                max_frequency_hz: 54_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,
                has_tuner: true,
                max_power_watts: Some(200),
            },
        },
    ),
    (
        0x03,
        RadioModelStatic {
            manufacturer: "Yaesu",
            model: "FT-710",
            protocol: Protocol::Yaesu,
            protocol_id: ProtocolIdStatic::YaesuCode(0x03),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FULL_HF,
                min_frequency_hz: 30_000,
                max_frequency_hz: 54_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,
                has_tuner: true,
                max_power_watts: Some(100),
            },
        },
    ),
    (
        0x04,
        RadioModelStatic {
            manufacturer: "Yaesu",
            model: "FTDX10",
            protocol: Protocol::Yaesu,
            protocol_id: ProtocolIdStatic::YaesuCode(0x04),
            capabilities: RadioCapabilitiesStatic {
                modes: MODES_FULL_HF,
                min_frequency_hz: 30_000,
                max_frequency_hz: 54_000_000,
                frequency_step_hz: 1,
                has_split: true,
                vfo_count: 2,
                has_tuner: true,
                max_power_watts: Some(100),
            },
        },
    ),
];
