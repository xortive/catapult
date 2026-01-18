//! Traffic monitor UI component

use std::collections::VecDeque;
use std::time::SystemTime;

use cat_mux::RadioHandle;
use egui::{Color32, RichText, Ui};

/// Source of traffic data
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrafficSource {
    /// Real radio on a serial port
    RealRadio {
        handle: RadioHandle,
        port: String,
    },
    /// Simulated radio
    SimulatedRadio {
        id: String,
    },
    /// Real amplifier on a serial port
    RealAmplifier {
        port: String,
    },
    /// Simulated amplifier (no real connection)
    SimulatedAmplifier,
}

impl TrafficSource {
    /// Check if this is a simulated source
    pub fn is_simulated(&self) -> bool {
        matches!(self, Self::SimulatedRadio { .. } | Self::SimulatedAmplifier)
    }
}

/// A single traffic entry
#[derive(Debug, Clone)]
pub struct TrafficEntry {
    /// Timestamp
    pub timestamp: SystemTime,
    /// Direction
    pub direction: TrafficDirection,
    /// Traffic source
    pub source: TrafficSource,
    /// Raw data
    pub data: Vec<u8>,
    /// Decoded representation (if available)
    pub decoded: Option<String>,
}

/// Traffic direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficDirection {
    /// Incoming from radio
    Incoming,
    /// Outgoing to amplifier
    Outgoing,
}

/// Traffic monitor state
pub struct TrafficMonitor {
    /// Traffic entries
    entries: VecDeque<TrafficEntry>,
    /// Maximum entries to keep
    max_entries: usize,
    /// Show hex view
    show_hex: bool,
    /// Show decoded view
    show_decoded: bool,
    /// Auto-scroll to bottom
    auto_scroll: bool,
    /// Filter by direction
    filter_direction: Option<TrafficDirection>,
    /// Show simulated traffic
    show_simulated: bool,
    /// Pause monitoring
    paused: bool,
}

impl TrafficMonitor {
    /// Create a new traffic monitor with display settings
    pub fn new(max_entries: usize, show_hex: bool, show_decoded: bool) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries),
            max_entries,
            show_hex,
            show_decoded,
            auto_scroll: true,
            filter_direction: None,
            show_simulated: true,
            paused: false,
        }
    }

    /// Add an incoming traffic entry from a real radio
    pub fn add_incoming(&mut self, radio: RadioHandle, data: &[u8]) {
        self.add_incoming_with_port(radio, String::new(), data);
    }

    /// Add an incoming traffic entry from a real radio with port info
    pub fn add_incoming_with_port(&mut self, radio: RadioHandle, port: String, data: &[u8]) {
        if self.paused {
            return;
        }

        self.add_entry(TrafficEntry {
            timestamp: SystemTime::now(),
            direction: TrafficDirection::Incoming,
            source: TrafficSource::RealRadio {
                handle: radio,
                port,
            },
            data: data.to_vec(),
            decoded: try_decode(data),
        });
    }

    /// Add an incoming traffic entry from a simulated radio
    pub fn add_simulated_incoming(&mut self, id: String, data: &[u8]) {
        if self.paused {
            return;
        }

        self.add_entry(TrafficEntry {
            timestamp: SystemTime::now(),
            direction: TrafficDirection::Incoming,
            source: TrafficSource::SimulatedRadio { id },
            data: data.to_vec(),
            decoded: try_decode(data),
        });
    }

    /// Add an outgoing traffic entry to real amplifier
    pub fn add_outgoing(&mut self, data: &[u8]) {
        self.add_outgoing_with_port(String::new(), data);
    }

    /// Add an outgoing traffic entry to real amplifier with port info
    pub fn add_outgoing_with_port(&mut self, port: String, data: &[u8]) {
        if self.paused {
            return;
        }

        self.add_entry(TrafficEntry {
            timestamp: SystemTime::now(),
            direction: TrafficDirection::Outgoing,
            source: TrafficSource::RealAmplifier { port },
            data: data.to_vec(),
            decoded: try_decode(data),
        });
    }

    /// Add an outgoing traffic entry to simulated amplifier
    pub fn add_simulated_outgoing(&mut self, data: &[u8]) {
        if self.paused {
            return;
        }

        self.add_entry(TrafficEntry {
            timestamp: SystemTime::now(),
            direction: TrafficDirection::Outgoing,
            source: TrafficSource::SimulatedAmplifier,
            data: data.to_vec(),
            decoded: try_decode(data),
        });
    }

    /// Add an entry
    fn add_entry(&mut self, entry: TrafficEntry) {
        if self.entries.len() >= self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Draw the traffic monitor UI
    pub fn draw(&mut self, ui: &mut Ui) {
        // Toolbar
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_hex, "Hex");
            ui.checkbox(&mut self.show_decoded, "Decoded");
            ui.separator();
            ui.checkbox(&mut self.auto_scroll, "Auto-scroll");
            ui.separator();

            if ui.button(if self.paused { "Resume" } else { "Pause" }).clicked() {
                self.paused = !self.paused;
            }

            if ui.button("Clear").clicked() {
                self.clear();
            }

            ui.separator();

            // Direction filter
            ui.label("Filter:");
            if ui
                .selectable_label(self.filter_direction.is_none(), "All")
                .clicked()
            {
                self.filter_direction = None;
            }
            if ui
                .selectable_label(
                    self.filter_direction == Some(TrafficDirection::Incoming),
                    "In",
                )
                .clicked()
            {
                self.filter_direction = Some(TrafficDirection::Incoming);
            }
            if ui
                .selectable_label(
                    self.filter_direction == Some(TrafficDirection::Outgoing),
                    "Out",
                )
                .clicked()
            {
                self.filter_direction = Some(TrafficDirection::Outgoing);
            }

            ui.separator();

            // Simulated traffic toggle
            ui.checkbox(&mut self.show_simulated, "Show SIM");
        });

        ui.separator();

        // Traffic list - collect filtered indices first for proper virtual scrolling
        let filtered_indices: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                // Direction filter
                let direction_match = self
                    .filter_direction
                    .is_none_or(|filter| entry.direction == filter);

                // Simulated filter
                let sim_match = self.show_simulated || !entry.source.is_simulated();

                direction_match && sim_match
            })
            .map(|(i, _)| i)
            .collect();

        let text_style = egui::TextStyle::Monospace;
        let row_height = ui.text_style_height(&text_style);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(self.auto_scroll)
            .show_rows(
                ui,
                row_height,
                filtered_indices.len(),
                |ui, row_range| {
                    for i in row_range {
                        if let Some(&entry_idx) = filtered_indices.get(i) {
                            if let Some(entry) = self.entries.get(entry_idx) {
                                self.draw_entry(ui, entry);
                            }
                        }
                    }
                },
            );
    }

    /// Draw a single traffic entry
    fn draw_entry(&self, ui: &mut Ui, entry: &TrafficEntry) {
        ui.horizontal(|ui| {
            // Timestamp
            let time = entry
                .timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| {
                    let secs = d.as_secs() % 86400;
                    let hours = secs / 3600;
                    let mins = (secs % 3600) / 60;
                    let secs = secs % 60;
                    let millis = d.subsec_millis();
                    format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis)
                })
                .unwrap_or_default();

            ui.label(RichText::new(time).color(Color32::GRAY).monospace());

            // Simulated badge
            if entry.source.is_simulated() {
                ui.label(
                    RichText::new("[SIM]")
                        .color(Color32::from_rgb(255, 165, 0)) // Orange
                        .strong()
                        .monospace(),
                );
            }

            // Direction indicator with source info
            match &entry.source {
                TrafficSource::RealRadio { .. } => {
                    ui.label(
                        RichText::new("[Radio→]")
                            .color(Color32::LIGHT_BLUE)
                            .monospace(),
                    );
                }
                TrafficSource::SimulatedRadio { id } => {
                    ui.label(
                        RichText::new(format!("[{}→]", id))
                            .color(Color32::from_rgb(100, 180, 255))
                            .monospace(),
                    );
                }
                TrafficSource::RealAmplifier { .. } => {
                    ui.label(
                        RichText::new("[→Amp]")
                            .color(Color32::LIGHT_GREEN)
                            .monospace(),
                    );
                }
                TrafficSource::SimulatedAmplifier => {
                    ui.label(
                        RichText::new("[→Amp]")
                            .color(Color32::from_rgb(100, 180, 100))
                            .monospace(),
                    );
                }
            }

            // Data
            if self.show_hex {
                let hex: String = entry
                    .data
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<_>>()
                    .join(" ");
                ui.label(RichText::new(hex).monospace());
            }

            if self.show_decoded {
                if let Some(decoded) = &entry.decoded {
                    ui.label(
                        RichText::new(format!("({})", decoded))
                            .color(Color32::YELLOW)
                            .monospace(),
                    );
                }
            }
        });
    }
}

/// Try to decode raw data into a human-readable string
fn try_decode(data: &[u8]) -> Option<String> {
    // Try ASCII (Kenwood/Elecraft)
    if let Ok(s) = std::str::from_utf8(data) {
        if s.chars().all(|c| c.is_ascii_graphic() || c == ';') {
            return Some(s.trim_end_matches(';').to_string());
        }
    }

    // Try CI-V frame
    if data.len() >= 6 && data[0] == 0xFE && data[1] == 0xFE {
        let to = data[2];
        let from = data[3];
        let cmd = data[4];
        return Some(format!("CI-V {:02X}→{:02X} cmd={:02X}", from, to, cmd));
    }

    // Yaesu 5-byte command
    if data.len() == 5 {
        let opcode = data[4];
        return Some(format!("Yaesu cmd={:02X}", opcode));
    }

    None
}
