//! Simulation panel for testing without physical hardware
//!
//! Provides UI controls to manage multiple virtual radios.

// Allow unused code - panel UI is implemented but not yet wired into the main app
#![allow(dead_code)]

use cat_protocol::{OperatingMode, Protocol, RadioDatabase};
use cat_sim::{SimulationContext, SimulationEvent};
use egui::{Color32, RichText, Ui};

/// Common amateur radio band presets (in Hz)
const BAND_PRESETS: &[(&str, u64)] = &[
    ("160m", 1_900_000),
    ("80m", 3_750_000),
    ("60m", 5_357_000),
    ("40m", 7_150_000),
    ("30m", 10_125_000),
    ("20m", 14_250_000),
    ("17m", 18_118_000),
    ("15m", 21_250_000),
    ("12m", 24_940_000),
    ("10m", 28_500_000),
    ("6m", 50_125_000),
    ("2m", 146_520_000),
];

/// Available operating modes for the UI
const MODES: &[OperatingMode] = &[
    OperatingMode::Lsb,
    OperatingMode::Usb,
    OperatingMode::Cw,
    OperatingMode::Am,
    OperatingMode::Fm,
    OperatingMode::Dig,
    OperatingMode::Rtty,
];

/// Simulation panel state
pub struct SimulationPanel {
    /// Simulation context managing virtual radios
    context: SimulationContext,
    /// UI state for the new radio form
    new_radio_name: String,
    /// Protocol for new radio
    new_radio_protocol: Protocol,
    /// Currently selected radio ID for editing
    selected_radio: Option<String>,
    /// Frequency input buffer for editing
    frequency_input: String,
    /// Whether the panel is expanded
    expanded: bool,
}

impl Default for SimulationPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulationPanel {
    /// Create a new simulation panel
    pub fn new() -> Self {
        Self {
            context: SimulationContext::new(),
            new_radio_name: String::new(),
            new_radio_protocol: Protocol::Kenwood,
            selected_radio: None,
            frequency_input: String::new(),
            expanded: true,
        }
    }

    /// Get a reference to the simulation context
    pub fn context(&self) -> &SimulationContext {
        &self.context
    }

    /// Get a mutable reference to the simulation context
    pub fn context_mut(&mut self) -> &mut SimulationContext {
        &mut self.context
    }

    /// Drain all pending simulation events
    pub fn drain_events(&mut self) -> Vec<SimulationEvent> {
        self.context.drain_events()
    }

    /// Draw the simulation panel UI
    pub fn draw(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Simulation");
            ui.label(RichText::new("DEBUG MODE").color(Color32::YELLOW).strong());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(if self.expanded { "Less" } else { "More" }).clicked() {
                    self.expanded = !self.expanded;
                }
            });
        });

        if !self.expanded {
            // Show compact summary when collapsed
            ui.label(format!("{} radios", self.context.radio_count()));
            return;
        }

        ui.add_space(8.0);
        ui.separator();

        // Add new radio section
        self.draw_add_radio_section(ui);

        ui.add_space(8.0);
        ui.separator();

        // Virtual radios list
        self.draw_radios_section(ui);
    }

    /// Draw the add radio section
    fn draw_add_radio_section(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Add Radio:");
            ui.text_edit_singleline(&mut self.new_radio_name)
                .on_hover_text("Name for the new virtual radio");

            egui::ComboBox::from_id_salt("new_radio_protocol")
                .selected_text(self.new_radio_protocol.name())
                .width(100.0)
                .show_ui(ui, |ui| {
                    for proto in [
                        Protocol::Kenwood,
                        Protocol::IcomCIV,
                        Protocol::Yaesu,
                        Protocol::YaesuAscii,
                        Protocol::Elecraft,
                        Protocol::FlexRadio,
                    ] {
                        ui.selectable_value(&mut self.new_radio_protocol, proto, proto.name());
                    }
                });

            if ui.button("+ Add").clicked() {
                let name = if self.new_radio_name.is_empty() {
                    format!("Radio {}", self.context.radio_count() + 1)
                } else {
                    self.new_radio_name.clone()
                };
                let id = self.context.add_radio(&name, self.new_radio_protocol);
                self.selected_radio = Some(id);
                self.new_radio_name.clear();
            }
        });
    }

    /// Draw the radios list section
    fn draw_radios_section(&mut self, ui: &mut Ui) {
        ui.heading("Virtual Radios");

        if self.context.radio_count() == 0 {
            ui.label(
                RichText::new("No virtual radios. Add one above.")
                    .color(Color32::GRAY)
                    .italics(),
            );
            return;
        }

        // Collect radio info to avoid borrow issues
        let radio_infos: Vec<_> = self
            .context
            .radios()
            .map(|(id, radio)| {
                (
                    id.clone(),
                    radio.id().to_string(),
                    radio.protocol(),
                    radio.model().cloned(),
                    radio.frequency_hz(),
                    radio.mode(),
                    radio.ptt(),
                )
            })
            .collect();

        for (id, name, protocol, model, freq_hz, mode, ptt) in radio_infos {
            let is_selected = self.selected_radio.as_deref() == Some(&id);

            ui.group(|ui| {
                // Radio header
                ui.horizontal(|ui| {
                    // Radio name and model (active state is shown in the sidebar radio list)
                    let model_name = model
                        .as_ref()
                        .map(|m| m.model.as_str())
                        .unwrap_or(protocol.name());
                    let header_text = format!("{} ({})", name, model_name);
                    if ui
                        .selectable_label(is_selected, RichText::new(header_text).strong())
                        .clicked()
                    {
                        self.selected_radio = if is_selected { None } else { Some(id.clone()) };
                        // Update frequency input when selecting
                        self.frequency_input = format_frequency(freq_hz);
                    }

                    // PTT indicator
                    if ptt {
                        ui.label(RichText::new("TX").color(Color32::RED).strong());
                    }

                    // Frequency display
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{:.3} MHz", freq_hz as f64 / 1_000_000.0))
                                .monospace()
                                .color(Color32::LIGHT_GREEN),
                        );
                        ui.label(RichText::new(mode_name(mode)).color(Color32::LIGHT_BLUE));
                    });
                });

                // Expanded controls when selected
                if is_selected {
                    ui.add_space(4.0);

                    // Model selector
                    ui.horizontal(|ui| {
                        ui.label("Model:");
                        let models = RadioDatabase::radios_for_protocol(protocol);
                        let current_model_name = model
                            .as_ref()
                            .map(|m| m.model.as_str())
                            .unwrap_or("Unknown");
                        egui::ComboBox::from_id_salt(format!("model_{}", id))
                            .selected_text(current_model_name)
                            .width(150.0)
                            .show_ui(ui, |ui| {
                                for m in &models {
                                    let display_name = format!("{} {}", m.manufacturer, m.model);
                                    if ui
                                        .selectable_label(
                                            model
                                                .as_ref()
                                                .map(|curr| curr.model == m.model)
                                                .unwrap_or(false),
                                            &display_name,
                                        )
                                        .clicked()
                                    {
                                        self.context.set_radio_model(&id, Some(m.clone()));
                                    }
                                }
                            });
                    });

                    // Band presets
                    ui.horizontal_wrapped(|ui| {
                        for (band_name, freq) in BAND_PRESETS {
                            if ui.small_button(*band_name).clicked() {
                                self.context.set_radio_frequency(&id, *freq);
                                self.frequency_input = format_frequency(*freq);
                            }
                        }
                    });

                    // Frequency controls
                    ui.horizontal(|ui| {
                        ui.label("Freq:");
                        let response = ui.text_edit_singleline(&mut self.frequency_input);
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            if let Some(hz) = parse_frequency(&self.frequency_input) {
                                self.context.set_radio_frequency(&id, hz);
                            }
                        }
                        if ui.button("Set").clicked() {
                            if let Some(hz) = parse_frequency(&self.frequency_input) {
                                self.context.set_radio_frequency(&id, hz);
                            }
                        }
                    });

                    // Tune buttons
                    ui.horizontal(|ui| {
                        for (label, delta) in [
                            ("-10k", -10_000i64),
                            ("-1k", -1_000),
                            ("-100", -100),
                            ("+100", 100),
                            ("+1k", 1_000),
                            ("+10k", 10_000),
                        ] {
                            if ui.small_button(label).clicked() {
                                let new_freq = (freq_hz as i64 + delta).max(0) as u64;
                                self.context.set_radio_frequency(&id, new_freq);
                                self.frequency_input = format_frequency(new_freq);
                            }
                        }
                    });

                    // Mode buttons
                    ui.horizontal_wrapped(|ui| {
                        for &m in MODES {
                            let is_current = mode == m;
                            let button = egui::Button::new(mode_name(m)).fill(if is_current {
                                Color32::from_rgb(60, 80, 60)
                            } else {
                                Color32::from_rgb(40, 40, 40)
                            });
                            if ui.add(button).clicked() {
                                self.context.set_radio_mode(&id, m);
                            }
                        }
                    });

                    // PTT and remove buttons
                    ui.horizontal(|ui| {
                        let ptt_button = egui::Button::new(if ptt { "TX ON" } else { "TX OFF" })
                            .fill(if ptt {
                                Color32::from_rgb(150, 50, 50)
                            } else {
                                Color32::from_rgb(50, 50, 50)
                            });
                        if ui.add(ptt_button).clicked() {
                            self.context.set_radio_ptt(&id, !ptt);
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Remove").clicked() {
                                self.context.remove_radio(&id);
                                self.selected_radio = None;
                            }
                        });
                    });
                }
            });
        }
    }
}

/// Format frequency for display (with separators)
fn format_frequency(hz: u64) -> String {
    let s = hz.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push('.');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Parse a frequency string, allowing various formats
fn parse_frequency(s: &str) -> Option<u64> {
    let cleaned: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    cleaned.parse().ok()
}

/// Get a display name for an operating mode
fn mode_name(mode: OperatingMode) -> &'static str {
    match mode {
        OperatingMode::Lsb => "LSB",
        OperatingMode::Usb => "USB",
        OperatingMode::Cw => "CW",
        OperatingMode::CwR => "CW-R",
        OperatingMode::Am => "AM",
        OperatingMode::Fm => "FM",
        OperatingMode::FmN => "FM-N",
        OperatingMode::Dig => "DIG",
        OperatingMode::DigU => "DIG-U",
        OperatingMode::DigL => "DIG-L",
        OperatingMode::Pkt => "PKT",
        OperatingMode::Data => "DATA",
        OperatingMode::DataU => "DATA-U",
        OperatingMode::DataL => "DATA-L",
        OperatingMode::Rtty => "RTTY",
        OperatingMode::RttyR => "RTTY-R",
    }
}
