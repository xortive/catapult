//! USB Vendor/Product ID database for known serial adapters
//!
//! This module contains VID/PID pairs for common USB-to-serial adapters
//! used with amateur radio equipment.

/// USB Vendor ID / Product ID pair
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsbId {
    pub vid: u16,
    pub pid: u16,
}

impl UsbId {
    pub const fn new(vid: u16, pid: u16) -> Self {
        Self { vid, pid }
    }
}

/// FTDI (Future Technology Devices International)
pub mod ftdi {
    use super::UsbId;

    pub const VID: u16 = 0x0403;

    pub const FT232R: UsbId = UsbId::new(VID, 0x6001);
    pub const FT232H: UsbId = UsbId::new(VID, 0x6014);
    pub const FT2232: UsbId = UsbId::new(VID, 0x6010);
    pub const FT4232: UsbId = UsbId::new(VID, 0x6011);
    pub const FT231X: UsbId = UsbId::new(VID, 0x6015);

    /// All known FTDI product IDs
    pub const ALL_PIDS: &[u16] = &[0x6001, 0x6010, 0x6011, 0x6014, 0x6015];
}

/// Silicon Labs CP210x
pub mod cp210x {
    use super::UsbId;

    pub const VID: u16 = 0x10C4;

    pub const CP2101: UsbId = UsbId::new(VID, 0xEA60);
    pub const CP2102: UsbId = UsbId::new(VID, 0xEA60);
    pub const CP2103: UsbId = UsbId::new(VID, 0xEA60);
    pub const CP2104: UsbId = UsbId::new(VID, 0xEA60);
    pub const CP2105: UsbId = UsbId::new(VID, 0xEA70);
    pub const CP2108: UsbId = UsbId::new(VID, 0xEA71);

    /// All known CP210x product IDs
    pub const ALL_PIDS: &[u16] = &[0xEA60, 0xEA70, 0xEA71];
}

/// WCH CH340/CH341
pub mod ch340 {
    use super::UsbId;

    pub const VID: u16 = 0x1A86;

    pub const CH340: UsbId = UsbId::new(VID, 0x7523);
    pub const CH341: UsbId = UsbId::new(VID, 0x5523);

    /// All known CH340/341 product IDs
    pub const ALL_PIDS: &[u16] = &[0x7523, 0x5523];
}

/// Prolific PL2303
pub mod prolific {
    use super::UsbId;

    pub const VID: u16 = 0x067B;

    pub const PL2303: UsbId = UsbId::new(VID, 0x2303);
    pub const PL2303HX: UsbId = UsbId::new(VID, 0x2303);

    /// All known Prolific product IDs
    pub const ALL_PIDS: &[u16] = &[0x2303];
}

/// Radio manufacturer-specific USB IDs
pub mod radio {
    use super::UsbId;

    /// Icom radios with built-in USB
    pub mod icom {
        use super::UsbId;

        pub const VID: u16 = 0x0C26;

        pub const IC_7300: UsbId = UsbId::new(VID, 0x0036);
        pub const IC_7610: UsbId = UsbId::new(VID, 0x0037);
        pub const IC_705: UsbId = UsbId::new(VID, 0x0044);
        pub const IC_9700: UsbId = UsbId::new(VID, 0x0042);

        pub const ALL_PIDS: &[u16] = &[0x0036, 0x0037, 0x0044, 0x0042];
    }

    /// Yaesu radios with built-in USB
    pub mod yaesu {
        use super::UsbId;

        pub const VID: u16 = 0x10C4; // Uses Silicon Labs chip

        // FT-991A, FTDX10, etc. typically use CP210x internally
        pub const FTDX101: UsbId = UsbId::new(VID, 0xEA60);
    }

    /// Kenwood radios with built-in USB
    pub mod kenwood {
        use super::UsbId;

        pub const VID: u16 = 0x0B28; // JVC Kenwood

        pub const TS_990S: UsbId = UsbId::new(VID, 0x0010);
        pub const TS_590SG: UsbId = UsbId::new(VID, 0x0011);

        pub const ALL_PIDS: &[u16] = &[0x0010, 0x0011];
    }
}

/// Check if a VID/PID is a known serial adapter
pub fn is_known_serial_adapter(vid: u16, pid: u16) -> bool {
    match vid {
        ftdi::VID => ftdi::ALL_PIDS.contains(&pid),
        cp210x::VID => cp210x::ALL_PIDS.contains(&pid),
        ch340::VID => ch340::ALL_PIDS.contains(&pid),
        prolific::VID => prolific::ALL_PIDS.contains(&pid),
        radio::icom::VID => radio::icom::ALL_PIDS.contains(&pid),
        radio::kenwood::VID => radio::kenwood::ALL_PIDS.contains(&pid),
        _ => false,
    }
}

/// Check if a VID/PID is a known radio with built-in USB
pub fn is_known_radio_usb(vid: u16, pid: u16) -> Option<&'static str> {
    match (vid, pid) {
        (radio::icom::VID, 0x0036) => Some("IC-7300"),
        (radio::icom::VID, 0x0037) => Some("IC-7610"),
        (radio::icom::VID, 0x0044) => Some("IC-705"),
        (radio::icom::VID, 0x0042) => Some("IC-9700"),
        (radio::kenwood::VID, 0x0010) => Some("TS-990S"),
        (radio::kenwood::VID, 0x0011) => Some("TS-590SG"),
        _ => None,
    }
}

/// Get adapter type name from VID
pub fn adapter_name(vid: u16) -> Option<&'static str> {
    match vid {
        ftdi::VID => Some("FTDI"),
        cp210x::VID => Some("CP210x"),
        ch340::VID => Some("CH340"),
        prolific::VID => Some("PL2303"),
        radio::icom::VID => Some("Icom USB"),
        radio::kenwood::VID => Some("Kenwood USB"),
        _ => None,
    }
}
