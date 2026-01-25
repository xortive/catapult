//! Unified event stream for the multiplexer
//!
//! All multiplexer events (radio state changes, traffic, amplifier activity)
//! are emitted through a single event channel. This enables a unified traffic
//! monitor and simplifies state observation.

use cat_protocol::{OperatingMode, Protocol};

use crate::amplifier::AmplifierChannelMeta;
use crate::channel::RadioChannelMeta;
use crate::state::{RadioHandle, SwitchingMode};

/// Unified event enum for all multiplexer activity
///
/// The traffic monitor and other observers receive all events through a single
/// channel, simplifying the architecture and ensuring consistent event ordering.
#[derive(Debug, Clone)]
pub enum MuxEvent {
    // -------------------------------------------------------------------------
    // Radio lifecycle events
    // -------------------------------------------------------------------------
    /// A radio has connected to the multiplexer
    RadioConnected {
        /// Handle for this radio
        handle: RadioHandle,
        /// Metadata about the radio
        meta: RadioChannelMeta,
    },

    /// A radio has disconnected from the multiplexer
    RadioDisconnected {
        /// Handle of the disconnected radio
        handle: RadioHandle,
    },

    /// A radio's state has changed (frequency, mode, or PTT)
    RadioStateChanged {
        /// Handle of the radio
        handle: RadioHandle,
        /// New frequency in Hz (if changed)
        freq: Option<u64>,
        /// New operating mode (if changed)
        mode: Option<OperatingMode>,
        /// New PTT state (if changed)
        ptt: Option<bool>,
    },

    /// The active radio has changed
    ActiveRadioChanged {
        /// Previous active radio (None if no radio was active)
        from: Option<RadioHandle>,
        /// New active radio
        to: RadioHandle,
    },

    // -------------------------------------------------------------------------
    // Traffic events (for traffic monitor)
    // -------------------------------------------------------------------------
    /// Data received from a radio (radio -> mux)
    RadioDataIn {
        /// Handle of the source radio
        handle: RadioHandle,
        /// Raw data bytes
        data: Vec<u8>,
        /// Protocol of the radio
        protocol: Protocol,
    },

    /// Data sent to a radio (mux -> radio)
    RadioDataOut {
        /// Handle of the target radio
        handle: RadioHandle,
        /// Raw data bytes
        data: Vec<u8>,
        /// Protocol of the radio
        protocol: Protocol,
    },

    /// Data sent to the amplifier (mux -> amp)
    AmpDataOut {
        /// Raw data bytes
        data: Vec<u8>,
        /// Protocol used for the amplifier
        protocol: Protocol,
    },

    /// Data received from the amplifier (amp -> mux)
    AmpDataIn {
        /// Raw data bytes
        data: Vec<u8>,
        /// Protocol used for the amplifier
        protocol: Protocol,
    },

    // -------------------------------------------------------------------------
    // Amplifier lifecycle events
    // -------------------------------------------------------------------------
    /// An amplifier has connected to the multiplexer
    AmpConnected {
        /// Metadata about the amplifier
        meta: AmplifierChannelMeta,
    },

    /// The amplifier has disconnected from the multiplexer
    AmpDisconnected,

    // -------------------------------------------------------------------------
    // Control events
    // -------------------------------------------------------------------------
    /// The switching mode has changed
    SwitchingModeChanged {
        /// New switching mode
        mode: SwitchingMode,
    },

    /// A radio switch was blocked due to lockout
    SwitchingBlocked {
        /// Radio that requested to become active
        requested: RadioHandle,
        /// Currently active radio
        current: RadioHandle,
        /// Time remaining in lockout (milliseconds)
        remaining_ms: u64,
    },

    /// An error occurred in the multiplexer
    Error {
        /// Source of the error
        source: String,
        /// Error message
        message: String,
    },
}

impl MuxEvent {
    /// Check if this is a traffic event (for traffic monitor filtering)
    pub fn is_traffic(&self) -> bool {
        matches!(
            self,
            MuxEvent::RadioDataIn { .. }
                | MuxEvent::RadioDataOut { .. }
                | MuxEvent::AmpDataOut { .. }
                | MuxEvent::AmpDataIn { .. }
        )
    }

    /// Check if this is a radio lifecycle event
    pub fn is_radio_lifecycle(&self) -> bool {
        matches!(
            self,
            MuxEvent::RadioConnected { .. }
                | MuxEvent::RadioDisconnected { .. }
                | MuxEvent::ActiveRadioChanged { .. }
        )
    }

    /// Check if this is an amplifier lifecycle event
    pub fn is_amp_lifecycle(&self) -> bool {
        matches!(
            self,
            MuxEvent::AmpConnected { .. } | MuxEvent::AmpDisconnected
        )
    }

    /// Get the radio handle if this event is associated with a specific radio
    pub fn radio_handle(&self) -> Option<RadioHandle> {
        match self {
            MuxEvent::RadioConnected { handle, .. }
            | MuxEvent::RadioDisconnected { handle }
            | MuxEvent::RadioStateChanged { handle, .. }
            | MuxEvent::RadioDataIn { handle, .. }
            | MuxEvent::RadioDataOut { handle, .. } => Some(*handle),
            MuxEvent::ActiveRadioChanged { to, .. } => Some(*to),
            MuxEvent::SwitchingBlocked { requested, .. } => Some(*requested),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_traffic_event_classification() {
        let radio_in = MuxEvent::RadioDataIn {
            handle: RadioHandle(1),
            data: vec![0x01, 0x02],
            protocol: Protocol::Kenwood,
        };
        assert!(radio_in.is_traffic());
        assert!(!radio_in.is_radio_lifecycle());

        let amp_out = MuxEvent::AmpDataOut {
            data: vec![0x03, 0x04],
            protocol: Protocol::Kenwood,
        };
        assert!(amp_out.is_traffic());

        let connected = MuxEvent::RadioConnected {
            handle: RadioHandle(1),
            meta: RadioChannelMeta::new_real(
                "Test".to_string(),
                "/dev/tty0".to_string(),
                Protocol::Kenwood,
                None,
            ),
        };
        assert!(!connected.is_traffic());
        assert!(connected.is_radio_lifecycle());
    }

    #[test]
    fn test_radio_handle_extraction() {
        let event = MuxEvent::RadioDataIn {
            handle: RadioHandle(42),
            data: vec![],
            protocol: Protocol::Kenwood,
        };
        assert_eq!(event.radio_handle(), Some(RadioHandle(42)));

        let amp_event = MuxEvent::AmpDataOut {
            data: vec![],
            protocol: Protocol::Kenwood,
        };
        assert_eq!(amp_event.radio_handle(), None);
    }
}
