//! Radio panel UI component

use cat_detect::DetectedRadio;
use cat_mux::RadioHandle;
use cat_protocol::Protocol;

/// UI panel for a single radio
pub struct RadioPanel {
    /// Radio handle in the multiplexer
    pub handle: RadioHandle,
    /// Display name
    pub name: String,
    /// Serial port
    pub port: String,
    /// Protocol (for future use in protocol-specific UI)
    #[allow(dead_code)]
    pub protocol: Protocol,
    /// Is expanded in UI (for future collapsible sections)
    #[allow(dead_code)]
    pub expanded: bool,
}

impl RadioPanel {
    /// Create a new radio panel from a detected radio
    pub fn new(handle: RadioHandle, detected: &DetectedRadio) -> Self {
        Self {
            handle,
            name: detected.model_name(),
            port: detected.port.clone(),
            protocol: detected.protocol,
            expanded: false,
        }
    }
}
