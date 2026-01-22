//! Application settings

use std::path::PathBuf;

use cat_protocol::Protocol;
use cat_sim::VirtualRadioConfig;
use egui::Ui;
use serde::{Deserialize, Serialize};

/// Saved COM port radio configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConfiguredRadio {
    /// Serial port path
    pub port: String,
    /// Protocol used
    pub protocol: Protocol,
    /// Model name (for display)
    pub model_name: String,
    /// Baud rate
    pub baud_rate: u32,
    /// CI-V address for Icom radios
    #[serde(default)]
    pub civ_address: Option<u8>,
}

/// Saved amplifier configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AmplifierSettings {
    /// Connection type: "com" or "simulated"
    pub connection_type: String,
    /// Protocol used
    pub protocol: Protocol,
    /// COM port (if connection_type is "com")
    #[serde(default)]
    pub port: String,
    /// Baud rate
    #[serde(default = "default_amp_baud")]
    pub baud_rate: u32,
    /// CI-V address for Icom amplifiers
    #[serde(default)]
    pub civ_address: u8,
}

fn default_amp_baud() -> u32 {
    9600
}

impl Default for AmplifierSettings {
    fn default() -> Self {
        Self {
            connection_type: "simulated".to_string(),
            protocol: Protocol::Kenwood,
            port: String::new(),
            baud_rate: 9600,
            civ_address: 0x00,
        }
    }
}

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// Lockout time in milliseconds
    pub lockout_ms: u64,
    /// Auto-scan on startup
    pub auto_scan: bool,
    /// Traffic monitor history size
    pub traffic_history_size: usize,
    /// Show hex in traffic monitor
    pub show_hex: bool,
    /// Show decoded in traffic monitor
    pub show_decoded: bool,
    /// Show diagnostic entries in traffic monitor (master toggle)
    #[serde(default)]
    pub show_diagnostics: bool,
    /// Show Debug-level diagnostics
    #[serde(default)]
    pub show_diagnostic_debug: bool,
    /// Show Info-level diagnostics
    #[serde(default = "default_true")]
    pub show_diagnostic_info: bool,
    /// Show Warning-level diagnostics
    #[serde(default = "default_true")]
    pub show_diagnostic_warning: bool,
    /// Show Error-level diagnostics
    #[serde(default = "default_true")]
    pub show_diagnostic_error: bool,
    /// Default baud rates to try
    pub baud_rates: Vec<u32>,
    /// Virtual radios to restore on startup
    #[serde(default)]
    pub virtual_radios: Vec<VirtualRadioConfig>,
    /// Configured COM port radios to restore on startup
    #[serde(default)]
    pub configured_radios: Vec<ConfiguredRadio>,
    /// Amplifier configuration
    #[serde(default)]
    pub amplifier: AmplifierSettings,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lockout_ms: 500,
            auto_scan: false,
            traffic_history_size: 1000,
            show_hex: true,
            show_decoded: true,
            show_diagnostics: false,
            show_diagnostic_debug: false,
            show_diagnostic_info: true,
            show_diagnostic_warning: true,
            show_diagnostic_error: true,
            baud_rates: vec![38400, 19200, 9600, 4800, 115200],
            virtual_radios: Vec::new(),
            configured_radios: Vec::new(),
            amplifier: AmplifierSettings::default(),
        }
    }
}

impl Settings {
    /// Get the XDG config directory for catapult
    /// Uses $XDG_CONFIG_HOME/catapult on Linux/macOS, falls back to ~/.config/catapult
    fn config_dir() -> Option<PathBuf> {
        // First try XDG_CONFIG_HOME environment variable
        if let Ok(xdg_config) = std::env::var("XDG_CONFIG_HOME") {
            let path = PathBuf::from(xdg_config);
            if path.is_absolute() {
                return Some(path.join("catapult"));
            }
        }

        // Fall back to ~/.config/catapult (XDG default)
        dirs::home_dir().map(|h| h.join(".config").join("catapult"))
    }

    /// Get the settings file path
    fn settings_path() -> Option<PathBuf> {
        Self::config_dir().map(|p| p.join("settings.json"))
    }

    /// Load settings from disk
    pub fn load() -> Self {
        Self::settings_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Save settings to disk
    pub fn save(&self) -> Result<(), String> {
        let path =
            Self::settings_path().ok_or_else(|| "Could not determine settings path".to_string())?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create settings directory: {}", e))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize settings: {}", e))?;

        std::fs::write(&path, json).map_err(|e| format!("Failed to write settings: {}", e))?;

        Ok(())
    }

    /// Check if settings have changed and auto-save if so
    /// Returns any error message for display
    fn auto_save_if_changed(&self, previous: &Settings) -> Option<String> {
        if self != previous {
            if let Err(e) = self.save() {
                return Some(e);
            }
        }
        None
    }

    /// Draw settings UI (auto-saves on change)
    /// Returns an error message if save failed
    pub fn draw(&mut self, ui: &mut Ui) -> Option<String> {
        let previous = self.clone();

        egui::Grid::new("settings_grid")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                // Lockout time
                ui.label("Lockout time (ms):");
                ui.add(egui::DragValue::new(&mut self.lockout_ms).range(0..=5000));
                ui.end_row();

                // Auto-scan
                ui.label("Auto-scan on startup:");
                ui.checkbox(&mut self.auto_scan, "");
                ui.end_row();

                // Traffic history
                ui.label("Traffic history:");
                ui.add(egui::DragValue::new(&mut self.traffic_history_size).range(100..=10000));
                ui.end_row();

                // Show hex
                ui.label("Show hex:");
                ui.checkbox(&mut self.show_hex, "");
                ui.end_row();

                // Show decoded
                ui.label("Show decoded:");
                ui.checkbox(&mut self.show_decoded, "");
                ui.end_row();
            });

        ui.add_space(16.0);

        ui.heading("Baud Rates");
        ui.label("Comma-separated list of baud rates to try:");

        let mut baud_str = self
            .baud_rates
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(", ");

        if ui.text_edit_singleline(&mut baud_str).changed() {
            self.baud_rates = baud_str
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
        }

        ui.add_space(16.0);

        // Show config file location
        if let Some(path) = Self::settings_path() {
            ui.label(
                egui::RichText::new(format!("Config: {}", path.display()))
                    .small()
                    .color(egui::Color32::GRAY),
            );
        }

        // Auto-save when settings change
        self.auto_save_if_changed(&previous)
    }
}
