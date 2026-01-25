//! Port enumeration and validation methods

use std::collections::HashSet;

use cat_detect::SerialPortInfo;

use super::CatapultApp;

impl CatapultApp {
    /// Refresh available ports (sync version for initialization)
    pub(super) fn refresh_ports(&mut self) {
        match self.scanner.enumerate_ports() {
            Ok(ports) => {
                self.available_ports = ports;
                self.validate_port_selections();
            }
            Err(e) => {
                self.report_warning("System", format!("Failed to enumerate ports: {}", e));
            }
        }
    }

    /// Validate port selections after port list changes
    pub(super) fn validate_port_selections(&mut self) {
        // Clear add_radio_port if it's no longer available
        if !self.add_radio_port.is_empty() {
            let port_exists = self
                .available_ports
                .iter()
                .any(|p| p.port == self.add_radio_port);
            if !port_exists {
                self.add_radio_port.clear();
            }
        }

        // Clear amp_port if it's no longer available or is now used by a radio
        if !self.amp_port.is_empty() {
            let port_exists = self.available_ports.iter().any(|p| p.port == self.amp_port);
            let in_use_by_radio = self.radio_ports_in_use().contains(&self.amp_port);
            if !port_exists || in_use_by_radio {
                self.amp_port.clear();
                if self.amp_data_tx.is_some() {
                    self.disconnect_amplifier();
                    self.set_status("Amplifier disconnected: port no longer available".into());
                }
                self.save_amplifier_settings();
            }
        }
    }

    /// Get set of ports currently used by radios
    pub(super) fn radio_ports_in_use(&self) -> HashSet<String> {
        self.radio_panels
            .iter()
            .filter(|p| !p.is_virtual())
            .map(|p| p.port.clone())
            .collect()
    }

    /// Get available ports for adding a new radio (excludes ports already used by radios)
    pub(super) fn available_radio_ports(&self) -> Vec<&SerialPortInfo> {
        let in_use = self.radio_ports_in_use();
        self.available_ports
            .iter()
            .filter(|p| !in_use.contains(&p.port))
            .collect()
    }

    /// Get available ports for amplifier (excludes ports used by radios)
    pub(super) fn available_amp_ports(&self) -> Vec<&SerialPortInfo> {
        let in_use = self.radio_ports_in_use();
        self.available_ports
            .iter()
            .filter(|p| !in_use.contains(&p.port))
            .collect()
    }

    /// Format a port label with product description
    pub(super) fn format_port_label(port: &SerialPortInfo) -> String {
        match &port.product {
            Some(product) => format!("{} ({})", port.port, product),
            None => port.port.clone(),
        }
    }
}
