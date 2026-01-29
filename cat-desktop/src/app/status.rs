//! Status messaging and save helpers

use std::time::Instant;

use cat_sim::VirtualRadioConfig;

use crate::settings::{AmplifierSettings, ConfiguredRadio};

use super::{AmplifierConnectionType, CatapultApp};

impl CatapultApp {
    /// Set a status message (also logs as Info via tracing, which goes to traffic monitor)
    pub(super) fn set_status(&mut self, msg: String) {
        self.status_message = Some((msg.clone(), Instant::now()));
        tracing::info!(source = "Status", "{}", msg);
    }

    /// Report an info message via tracing (shows in console and traffic monitor)
    pub(super) fn report_info(&mut self, source: &str, message: impl Into<String>) {
        let message = message.into();
        tracing::info!(source = source, "{}", message);
    }

    /// Report a warning via tracing (shows in console, traffic monitor, and status bar)
    pub(super) fn report_warning(&mut self, source: &str, message: impl Into<String>) {
        let message = message.into();
        self.status_message = Some((format!("{}: {}", source, message), Instant::now()));
        tracing::warn!(source = source, "{}", message);
    }

    /// Report an error via tracing (shows in console, traffic monitor, and status bar)
    pub(super) fn report_err(&mut self, source: &str, message: impl Into<String>) {
        let message = message.into();
        self.status_message = Some((format!("{}: {}", source, message), Instant::now()));
        tracing::error!(source = source, "{}", message);
    }

    /// Handle a settings save error
    pub(super) fn handle_save_error(&mut self, error: String) {
        self.report_err("Settings", error);
    }

    /// Save current virtual radios to settings
    ///
    /// Gets state from SimulationPanel's radio_states since the actual VirtualRadio
    /// instances are owned by the actor tasks.
    pub(super) fn save_virtual_radios(&mut self) {
        // Get configs from SimulationPanel's display state
        let configs: Vec<VirtualRadioConfig> = self.simulation_panel.get_radio_configs().collect();

        if self.settings.virtual_radios != configs {
            self.settings.virtual_radios = configs;
            if let Err(e) = self.settings.save() {
                self.handle_save_error(e);
            }
        }
    }

    /// Save current amplifier settings
    pub(super) fn save_amplifier_settings(&mut self) {
        let amp_settings = AmplifierSettings {
            connection_type: match self.amp_connection_type {
                AmplifierConnectionType::ComPort => "com".to_string(),
                AmplifierConnectionType::Simulated => "simulated".to_string(),
            },
            protocol: self.amp_protocol,
            port: self.amp_port.clone(),
            baud_rate: self.amp_baud,
            civ_address: self.amp_civ_address,
            flow_control: self.amp_flow_control,
        };

        if self.settings.amplifier != amp_settings {
            self.settings.amplifier = amp_settings;
            if let Err(e) = self.settings.save() {
                self.handle_save_error(e);
            }
        }
    }

    /// Save current configured COM radios to settings
    pub(super) fn save_configured_radios(&mut self) {
        let configs: Vec<ConfiguredRadio> = self
            .radio_panels
            .iter()
            .filter(|p| !p.is_virtual())
            .map(|p| ConfiguredRadio {
                port: p.port.clone(),
                protocol: p.protocol,
                model_name: p.name.clone(),
                baud_rate: p.baud_rate,
                civ_address: p.civ_address,
                flow_control: p.flow_control.into(),
            })
            .collect();

        if self.settings.configured_radios != configs {
            self.settings.configured_radios = configs;
            if let Err(e) = self.settings.save() {
                self.handle_save_error(e);
            }
        }
    }
}
