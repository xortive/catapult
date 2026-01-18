//! Application settings

use std::path::PathBuf;

use egui::Ui;
use serde::{Deserialize, Serialize};

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
    /// Default baud rates to try
    pub baud_rates: Vec<u32>,
    /// Debug mode - enables simulated radio without hardware
    #[serde(default)]
    pub debug_mode: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lockout_ms: 500,
            auto_scan: false,
            traffic_history_size: 1000,
            show_hex: true,
            show_decoded: true,
            baud_rates: vec![38400, 19200, 9600, 4800, 115200],
            debug_mode: false,
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
    pub fn save(&self) {
        if let Some(path) = Self::settings_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(path, json);
            }
        }
    }

    /// Check if settings have changed and auto-save if so
    fn auto_save_if_changed(&self, previous: &Settings) {
        if self != previous {
            self.save();
        }
    }

    /// Draw settings UI (auto-saves on change)
    pub fn draw(&mut self, ui: &mut Ui) {
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

                // Debug mode
                ui.label("Debug mode:");
                ui.checkbox(&mut self.debug_mode, "");
                ui.end_row();
            });

        if self.debug_mode {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "Debug mode enables simulated radio for testing without hardware",
                )
                .color(egui::Color32::YELLOW)
                .small(),
            );
        }

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
        self.auto_save_if_changed(&previous);
    }
}
