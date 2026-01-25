//! Simulation panel for testing without physical hardware
//!
//! Provides UI controls to manage multiple virtual radios.
//! Returns SimulationAction for App to handle lifecycle (add/remove).
//! State updates come from mux events, and commands are sent via channels.

// Allow dead_code - UI code is not yet fully wired into main app
#![allow(dead_code)]

use std::collections::HashMap;

use cat_protocol::{OperatingMode, Protocol, ProtocolId, RadioDatabase, RadioModel};
use egui::{Color32, RichText, Ui};
use tokio::sync::mpsc;

use cat_sim::VirtualRadioCommand;

/// Actions returned from SimulationPanel for App to execute
#[derive(Debug, Clone)]
pub enum SimulationAction {
    /// Add a new virtual radio
    AddRadio { name: String, protocol: Protocol },
    /// Remove an existing virtual radio
    RemoveRadio { sim_id: String },
}

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

/// State of a virtual radio for display purposes
#[derive(Debug, Clone)]
pub struct VirtualRadioDisplayState {
    /// Display name
    pub name: String,
    /// Protocol used
    pub protocol: Protocol,
    /// Radio model (if known)
    pub model: Option<RadioModel>,
    /// Current frequency in Hz
    pub frequency_hz: u64,
    /// Current operating mode
    pub mode: OperatingMode,
    /// PTT active state
    pub ptt: bool,
}

impl VirtualRadioDisplayState {
    /// Create a new display state with default values
    pub fn new(name: String, protocol: Protocol) -> Self {
        Self {
            name,
            protocol,
            model: RadioDatabase::default_for_protocol(protocol),
            frequency_hz: 14_250_000, // 20m default
            mode: OperatingMode::Usb,
            ptt: false,
        }
    }
}

/// Simulation panel state
pub struct SimulationPanel {
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
    /// Display state for each virtual radio (keyed by sim_id)
    radio_states: HashMap<String, VirtualRadioDisplayState>,
    /// Command senders for each virtual radio (keyed by sim_id)
    radio_commands: HashMap<String, mpsc::Sender<VirtualRadioCommand>>,
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
            new_radio_name: String::new(),
            new_radio_protocol: Protocol::Kenwood,
            selected_radio: None,
            frequency_input: String::new(),
            expanded: true,
            radio_states: HashMap::new(),
            radio_commands: HashMap::new(),
        }
    }

    /// Register a virtual radio after it has been added by App
    ///
    /// Called by App::add_virtual_radio() after spawning the actor.
    /// Also selects the new radio for editing.
    pub fn register_radio(
        &mut self,
        sim_id: String,
        name: String,
        protocol: Protocol,
        cmd_tx: mpsc::Sender<VirtualRadioCommand>,
    ) {
        self.radio_states
            .insert(sim_id.clone(), VirtualRadioDisplayState::new(name, protocol));
        self.radio_commands.insert(sim_id.clone(), cmd_tx);
        self.selected_radio = Some(sim_id);
    }

    /// Unregister a virtual radio
    ///
    /// Called by App::remove_virtual_radio().
    pub fn unregister_radio(&mut self, sim_id: &str) {
        self.radio_states.remove(sim_id);
        self.radio_commands.remove(sim_id);
        if self.selected_radio.as_deref() == Some(sim_id) {
            self.selected_radio = None;
        }
    }

    /// Update a radio's display state from mux events
    pub fn update_radio_state(
        &mut self,
        sim_id: &str,
        frequency_hz: Option<u64>,
        mode: Option<OperatingMode>,
        ptt: Option<bool>,
    ) {
        if let Some(state) = self.radio_states.get_mut(sim_id) {
            if let Some(hz) = frequency_hz {
                state.frequency_hz = hz;
            }
            if let Some(m) = mode {
                state.mode = m;
            }
            if let Some(p) = ptt {
                state.ptt = p;
            }
        }
    }

    /// Update a radio's model
    pub fn update_radio_model(&mut self, sim_id: &str, model: Option<RadioModel>) {
        if let Some(state) = self.radio_states.get_mut(sim_id) {
            state.model = model;
        }
    }

    /// Get the number of registered virtual radios
    pub fn radio_count(&self) -> usize {
        self.radio_states.len()
    }

    /// Check if a sim_id is a registered virtual radio
    pub fn has_radio(&self, sim_id: &str) -> bool {
        self.radio_states.contains_key(sim_id)
    }

    /// Get radio configurations for saving to settings
    ///
    /// Returns an iterator of VirtualRadioConfig from the current display state.
    pub fn get_radio_configs(&self) -> impl Iterator<Item = cat_sim::VirtualRadioConfig> + '_ {
        self.radio_states.iter().map(|(_, state)| cat_sim::VirtualRadioConfig {
            id: state.name.clone(),
            protocol: state.protocol,
            model_name: state.model.as_ref().map(|m| m.model.clone()),
            initial_frequency_hz: state.frequency_hz,
            initial_mode: state.mode,
            civ_address: state.model.as_ref().and_then(|m| {
                if let ProtocolId::CivAddress(addr) = &m.protocol_id {
                    Some(*addr)
                } else {
                    None
                }
            }),
        })
    }

    /// Send a command to a virtual radio
    ///
    /// This can be called from app.rs for the radio panel UI controls.
    pub fn send_command(&self, sim_id: &str, cmd: VirtualRadioCommand) {
        if let Some(tx) = self.radio_commands.get(sim_id) {
            let _ = tx.try_send(cmd);
        }
    }

    /// Render UI and return any actions to execute
    pub fn ui(&mut self, ui: &mut Ui) -> Option<SimulationAction> {
        ui.horizontal(|ui| {
            ui.heading("Simulation");
            ui.label(RichText::new("DEBUG MODE").color(Color32::YELLOW).strong());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(if self.expanded { "Less" } else { "More" })
                    .clicked()
                {
                    self.expanded = !self.expanded;
                }
            });
        });

        if !self.expanded {
            // Show compact summary when collapsed
            ui.label(format!("{} radios", self.radio_count()));
            return None;
        }

        ui.add_space(8.0);
        ui.separator();

        // Add new radio section
        let add_action = self.draw_add_radio_section(ui);

        ui.add_space(8.0);
        ui.separator();

        // Virtual radios list
        let remove_action = self.draw_radios_section(ui);

        // Return the first action (add takes precedence)
        add_action.or(remove_action)
    }

    /// Draw the add radio section, returning AddRadio action if button clicked
    fn draw_add_radio_section(&mut self, ui: &mut Ui) -> Option<SimulationAction> {
        let mut action = None;

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
                    format!("Radio {}", self.radio_count() + 1)
                } else {
                    self.new_radio_name.clone()
                };
                action = Some(SimulationAction::AddRadio {
                    name,
                    protocol: self.new_radio_protocol,
                });
                self.new_radio_name.clear();
            }
        });

        action
    }

    /// Draw the radios list section, returning RemoveRadio action if button clicked
    fn draw_radios_section(&mut self, ui: &mut Ui) -> Option<SimulationAction> {
        ui.heading("Virtual Radios");

        if self.radio_states.is_empty() {
            ui.label(
                RichText::new("No virtual radios. Add one above.")
                    .color(Color32::GRAY)
                    .italics(),
            );
            return None;
        }

        // Track which radio to remove (if any)
        let mut remove_id: Option<String> = None;

        // Collect radio info to avoid borrow issues
        let radio_infos: Vec<_> = self
            .radio_states
            .iter()
            .map(|(id, state)| {
                (
                    id.clone(),
                    state.name.clone(),
                    state.protocol,
                    state.model.clone(),
                    state.frequency_hz,
                    state.mode,
                    state.ptt,
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
                                        self.send_command(
                                            &id,
                                            VirtualRadioCommand::SetModel(Some(m.clone())),
                                        );
                                        // Update local state immediately for responsive UI
                                        if let Some(state) = self.radio_states.get_mut(&id) {
                                            state.model = Some(m.clone());
                                        }
                                    }
                                }
                            });
                    });

                    // Band presets
                    ui.horizontal_wrapped(|ui| {
                        for (band_name, freq) in BAND_PRESETS {
                            if ui.small_button(*band_name).clicked() {
                                self.send_command(&id, VirtualRadioCommand::SetFrequency(*freq));
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
                                self.send_command(&id, VirtualRadioCommand::SetFrequency(hz));
                            }
                        }
                        if ui.button("Set").clicked() {
                            if let Some(hz) = parse_frequency(&self.frequency_input) {
                                self.send_command(&id, VirtualRadioCommand::SetFrequency(hz));
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
                                self.send_command(&id, VirtualRadioCommand::SetFrequency(new_freq));
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
                                self.send_command(&id, VirtualRadioCommand::SetMode(m));
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
                            self.send_command(&id, VirtualRadioCommand::SetPtt(!ptt));
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Remove").clicked() {
                                remove_id = Some(id.clone());
                            }
                        });
                    });
                }
            });
        }

        // Return remove action if button was clicked
        remove_id.map(|sim_id| {
            self.selected_radio = None;
            SimulationAction::RemoveRadio { sim_id }
        })
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
