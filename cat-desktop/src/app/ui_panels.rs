//! UI panel drawing methods

use cat_mux::{MuxActorCommand, RadioHandle, SwitchingMode};
use cat_protocol::{OperatingMode, Protocol};
use cat_sim::VirtualRadioCommand;
use egui::{Color32, RichText, Ui};

use crate::traffic_monitor::ExportAction;

use super::{mode_name, AmplifierConnectionType, CatapultApp};

impl CatapultApp {
    /// Draw the toolbar
    pub(super) fn draw_toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // Console toggle button
            if self.show_traffic_monitor {
                if ui.button("Hide Console").clicked() {
                    self.show_traffic_monitor = false;
                }
            } else if ui.button("Show Console").clicked() {
                self.show_traffic_monitor = true;
            }

            ui.separator();

            if ui.button("Settings").clicked() {
                self.show_settings = !self.show_settings;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Active radio indicator
                let has_active = self.active_radio.is_some();
                if has_active {
                    ui.label(RichText::new("*").color(Color32::GREEN).size(16.0));
                    ui.label("Active");
                } else {
                    ui.label(RichText::new("o").color(Color32::GRAY).size(16.0));
                    ui.label("No radio");
                }

                ui.separator();

                // Amplifier status
                match self.amp_connection_type {
                    AmplifierConnectionType::ComPort => {
                        if self.amp_data_tx.is_some() {
                            ui.label(RichText::new("Amp: Connected").color(Color32::GREEN));
                        } else {
                            ui.label(RichText::new("Amp: Disconnected").color(Color32::GRAY));
                        }
                    }
                    AmplifierConnectionType::Simulated => {
                        ui.label(
                            RichText::new("Amp: Simulated").color(Color32::from_rgb(100, 180, 255)),
                        );
                    }
                }

                ui.separator();

                // Status message
                if let Some((msg, _)) = &self.status_message {
                    ui.label(msg);
                }
            });
        });
    }

    /// Draw the radio list panel (unified COM and Virtual radios)
    pub(super) fn draw_radio_panel(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Radios");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Add Radio dropdown menu
                ui.menu_button("+", |ui| {
                    ui.label(RichText::new("Add Radio:").small());

                    // Collect available ports into owned data to avoid borrow conflicts
                    // PortInfo includes both real serial ports and virtual ports from settings
                    let available_ports: Vec<(String, String, bool, Option<Protocol>)> = self
                        .available_radio_ports()
                        .into_iter()
                        .map(|p| {
                            (
                                p.port_name(),
                                p.display_label(),
                                p.is_virtual(),
                                p.virtual_protocol(),
                            )
                        })
                        .collect();

                    if available_ports.is_empty() {
                        ui.label(
                            RichText::new("No ports available")
                                .color(Color32::GRAY)
                                .small(),
                        );
                        ui.label(
                            RichText::new("Configure virtual ports in Settings")
                                .color(Color32::GRAY)
                                .small(),
                        );
                    } else {
                        // Port dropdown
                        let selected_label = if self.add_radio_port.is_empty() {
                            "Select port...".to_string()
                        } else {
                            // Find the label for the selected port
                            available_ports
                                .iter()
                                .find(|(port, _, _, _)| *port == self.add_radio_port)
                                .map(|(_, label, _, _)| label.clone())
                                .unwrap_or_else(|| self.add_radio_port.clone())
                        };

                        let prev_port = self.add_radio_port.clone();
                        egui::ComboBox::from_id_salt("add_radio_port")
                            .selected_text(&selected_label)
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                for (port_name, label, _, _) in &available_ports {
                                    ui.selectable_value(
                                        &mut self.add_radio_port,
                                        port_name.clone(),
                                        label,
                                    );
                                }
                            });

                        // Clear model when port changes (protocol set via probing)
                        if self.add_radio_port != prev_port {
                            self.add_radio_model.clear();
                        }

                        // Protocol dropdown
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Protocol:").small());
                            egui::ComboBox::from_id_salt("add_radio_protocol")
                                .selected_text(self.add_radio_protocol.name())
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
                                        ui.selectable_value(
                                            &mut self.add_radio_protocol,
                                            proto,
                                            proto.name(),
                                        );
                                    }
                                });
                        });

                        // Baud rate dropdown (only relevant for real COM ports)
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Baud:").small());
                            egui::ComboBox::from_id_salt("add_radio_baud")
                                .selected_text(format!("{}", self.add_radio_baud))
                                .width(80.0)
                                .show_ui(ui, |ui| {
                                    for &baud in &[4800u32, 9600, 19200, 38400, 57600, 115200] {
                                        ui.selectable_value(
                                            &mut self.add_radio_baud,
                                            baud,
                                            format!("{}", baud),
                                        );
                                    }
                                });
                        });

                        // CI-V address for Icom protocol
                        if self.add_radio_protocol == Protocol::IcomCIV {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("CI-V:").small());
                                let mut addr_str = format!("{:02X}", self.add_radio_civ_address);
                                let response = ui.add(
                                    egui::TextEdit::singleline(&mut addr_str).desired_width(40.0),
                                );
                                if response.changed() {
                                    if let Ok(addr) =
                                        u8::from_str_radix(addr_str.trim_start_matches("0x"), 16)
                                    {
                                        self.add_radio_civ_address = addr;
                                    }
                                }
                            });
                        }

                        // Detect Model button and detected model display
                        ui.horizontal(|ui| {
                            let can_probe = !self.add_radio_port.is_empty() && !self.probing;
                            if self.probing {
                                ui.spinner();
                            } else if ui
                                .add_enabled(can_probe, egui::Button::new("Detect Model"))
                                .on_hover_text("Query radio for model identification using selected protocol")
                                .clicked()
                            {
                                self.probe_selected_port();
                            }
                            if !self.add_radio_model.is_empty() {
                                ui.label(
                                    RichText::new(&self.add_radio_model)
                                        .small()
                                        .color(Color32::GREEN),
                                );
                            }
                        });

                        // Add Radio button
                        let can_add = !self.add_radio_port.is_empty() && !self.probing;
                        if ui
                            .add_enabled(can_add, egui::Button::new("Add"))
                            .on_hover_text("Add radio")
                            .clicked()
                        {
                            self.add_radio_from_port();
                            // Reset the model field after adding
                            self.add_radio_model.clear();
                            ui.close_menu();
                        }
                    }
                });
            });
        });

        if self.radio_panels.is_empty() {
            ui.label("No radios. Click '+' to add a radio.");
            return;
        }

        // Get active radio handle for comparison
        let active_handle = self.active_radio;

        // Collect radio info from local RadioPanel state
        let radio_info: Vec<_> = self
            .radio_panels
            .iter()
            .enumerate()
            .map(|(idx, panel)| {
                // Read state from local RadioPanel fields
                let freq = panel.frequency_hz.unwrap_or(0);
                let mode = panel.mode.unwrap_or(OperatingMode::Usb);
                let freq_display = if freq > 0 {
                    format!("{:.3} MHz", freq as f64 / 1_000_000.0)
                } else {
                    "---.--- MHz".to_string()
                };
                let mode_display = panel.mode.map(mode_name).unwrap_or("---").to_string();

                (
                    idx,
                    panel.handle,
                    panel.name.clone(),
                    panel.port.clone(),
                    panel.is_virtual(),
                    panel.sim_id().map(String::from),
                    panel.expanded,
                    panel.protocol,
                    freq_display,
                    mode_display,
                    panel.ptt,
                    freq,
                    mode,
                )
            })
            .collect::<Vec<_>>();

        let mut selected_handle: Option<RadioHandle> = None;
        let mut toggle_expanded_idx = None;
        let mut remove_radio_idx = None;
        let mut freq_change: Option<(String, u64)> = None;
        let mut mode_change: Option<(String, OperatingMode)> = None;
        let mut ptt_change: Option<(String, bool)> = None;

        for (
            idx,
            handle,
            name,
            port,
            is_virtual,
            sim_id,
            expanded,
            protocol,
            freq_display,
            mode_display,
            ptt,
            freq_hz,
            mode,
        ) in &radio_info
        {
            let is_active = handle.is_some() && active_handle == *handle;

            // Determine background color based on state
            let bg_color = if *ptt {
                if *is_virtual {
                    Color32::from_rgb(80, 40, 20) // Red-orange tint for virtual
                } else {
                    Color32::from_rgb(80, 30, 30) // Red tint for COM
                }
            } else if is_active {
                if *is_virtual {
                    Color32::from_rgb(60, 50, 30)
                } else {
                    Color32::from_rgb(40, 60, 40)
                }
            } else if *is_virtual {
                Color32::from_rgb(40, 35, 25)
            } else {
                Color32::from_rgb(30, 30, 30)
            };

            egui::Frame::none()
                .fill(bg_color)
                .rounding(4.0)
                .inner_margin(8.0)
                .outer_margin(4.0)
                .show(ui, |ui| {
                    // Top row: SIM badge (for virtual radios), TX indicator, and Select/Expand button
                    ui.horizontal(|ui| {
                        // SIM badge only for virtual radios
                        if *is_virtual {
                            ui.label(
                                RichText::new("[SIM]")
                                    .color(Color32::from_rgb(255, 165, 0)) // Orange for virtual
                                    .strong()
                                    .size(10.0),
                            );
                        }

                        if *ptt {
                            ui.label(
                                RichText::new("* TX")
                                    .color(Color32::from_rgb(255, 80, 80))
                                    .strong()
                                    .size(14.0),
                            );
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !is_active && ui.button("Select").clicked() {
                                selected_handle = *handle;
                            }
                            // Expand/collapse toggle
                            if ui.button(if *expanded { "Less" } else { "More" }).clicked() {
                                toggle_expanded_idx = Some(*idx);
                            }
                        });
                    });

                    // Frequency - large and prominent
                    ui.label(
                        RichText::new(freq_display)
                            .size(22.0)
                            .strong()
                            .color(Color32::WHITE),
                    );

                    // Mode - prominent
                    ui.label(
                        RichText::new(mode_display)
                            .size(16.0)
                            .color(Color32::from_rgb(180, 180, 255)),
                    );

                    ui.add_space(4.0);

                    // Radio name and port/protocol - small, secondary
                    ui.horizontal(|ui| {
                        if is_active {
                            ui.label(RichText::new("*").color(Color32::GREEN).size(10.0));
                        }
                        let detail = if *is_virtual {
                            protocol.name()
                        } else {
                            port.as_str()
                        };
                        ui.label(
                            RichText::new(format!("{} - {}", name, detail))
                                .color(Color32::GRAY)
                                .size(11.0),
                        );
                    });

                    // Expanded controls for virtual radios
                    if *is_virtual && *expanded {
                        if let Some(sim_id) = sim_id {
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(4.0);

                            // Band presets
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Band:").small());
                                for (band_name, band_freq) in &[
                                    ("160m", 1_900_000u64),
                                    ("80m", 3_750_000),
                                    ("40m", 7_150_000),
                                    ("20m", 14_250_000),
                                    ("15m", 21_250_000),
                                    ("10m", 28_500_000),
                                    ("6m", 50_125_000),
                                    ("2m", 146_520_000),
                                ] {
                                    if ui.small_button(*band_name).clicked() {
                                        freq_change = Some((sim_id.clone(), *band_freq));
                                    }
                                }
                            });

                            // Tune buttons
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Tune:").small());
                                for (label, delta) in [
                                    ("-10k", -10_000i64),
                                    ("-1k", -1_000),
                                    ("+1k", 1_000),
                                    ("+10k", 10_000),
                                ] {
                                    if ui.small_button(label).clicked() {
                                        let new_freq = (*freq_hz as i64 + delta).max(0) as u64;
                                        freq_change = Some((sim_id.clone(), new_freq));
                                    }
                                }
                            });

                            // Mode buttons
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Mode:").small());
                                for m in [
                                    OperatingMode::Lsb,
                                    OperatingMode::Usb,
                                    OperatingMode::Cw,
                                    OperatingMode::Am,
                                    OperatingMode::Fm,
                                    OperatingMode::Dig,
                                ] {
                                    let is_current = *mode == m;
                                    let button = egui::Button::new(mode_name(m)).small().fill(
                                        if is_current {
                                            Color32::from_rgb(60, 80, 60)
                                        } else {
                                            Color32::from_rgb(40, 40, 40)
                                        },
                                    );
                                    if ui.add(button).clicked() {
                                        mode_change = Some((sim_id.clone(), m));
                                    }
                                }
                            });

                            // PTT and Remove buttons
                            ui.horizontal(|ui| {
                                let ptt_text = if *ptt { "TX ON" } else { "TX OFF" };
                                let ptt_button = egui::Button::new(ptt_text).fill(if *ptt {
                                    Color32::from_rgb(150, 50, 50)
                                } else {
                                    Color32::from_rgb(50, 50, 50)
                                });
                                if ui.add(ptt_button).clicked() {
                                    ptt_change = Some((sim_id.clone(), !*ptt));
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .button(
                                                RichText::new("Remove")
                                                    .color(Color32::from_rgb(255, 100, 100)),
                                            )
                                            .clicked()
                                        {
                                            remove_radio_idx = Some(*idx);
                                        }
                                    },
                                );
                            });
                        }
                    }

                    // Expanded controls for COM radios
                    if !is_virtual && *expanded {
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .button(
                                            RichText::new("Remove")
                                                .color(Color32::from_rgb(255, 100, 100)),
                                        )
                                        .clicked()
                                    {
                                        remove_radio_idx = Some(*idx);
                                    }
                                },
                            );
                        });
                    }
                });
        }

        // Handle deferred actions
        if let Some(handle) = selected_handle {
            // Send SetActiveRadio to mux actor
            self.send_mux_command(MuxActorCommand::SetActiveRadio { handle }, "SetActiveRadio");
        }
        if let Some(idx) = toggle_expanded_idx {
            self.radio_panels[idx].expanded = !self.radio_panels[idx].expanded;
        }
        if let Some((sim_id, freq)) = freq_change {
            self.simulation_panel
                .send_command(&sim_id, VirtualRadioCommand::SetFrequency(freq));
        }
        if let Some((sim_id, m)) = mode_change {
            self.simulation_panel
                .send_command(&sim_id, VirtualRadioCommand::SetMode(m));
        }
        if let Some((sim_id, active)) = ptt_change {
            self.simulation_panel
                .send_command(&sim_id, VirtualRadioCommand::SetPtt(active));
        }
        if let Some(idx) = remove_radio_idx {
            // Get the handle from the panel
            if let Some(handle) = self.radio_panels.get(idx).and_then(|p| p.handle) {
                // Use unified remove_radio method
                self.remove_radio(handle);
            }
        }
    }

    /// Draw the switching mode panel
    pub(super) fn draw_switching_panel(&mut self, ui: &mut Ui) {
        // Read from local state
        let mut mode = self.switching_mode;

        egui::Grid::new("switch_config")
            .num_columns(2)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                ui.label("Mode:");
                egui::ComboBox::from_id_salt("switch_mode")
                    .selected_text(mode.name())
                    .show_ui(ui, |ui| {
                        for m in [
                            SwitchingMode::FrequencyTriggered,
                            SwitchingMode::Automatic,
                            SwitchingMode::Manual,
                        ] {
                            if ui.selectable_value(&mut mode, m, m.name()).changed() {
                                // Send SetSwitchingMode to mux actor
                                self.switching_mode = mode;
                                self.send_mux_command(
                                    MuxActorCommand::SetSwitchingMode { mode },
                                    "SetSwitchingMode",
                                );
                            }
                        }
                    });
                ui.end_row();
            });

        ui.label(
            RichText::new(mode.description())
                .color(Color32::GRAY)
                .size(11.0),
        );
    }

    /// Draw the traffic monitor panel
    pub(super) fn draw_traffic_panel(&mut self, ui: &mut Ui) {
        ui.heading("Traffic Monitor");

        // Draw and handle export actions
        if let Some(action) =
            self.traffic_monitor
                .draw(ui, self.settings.show_hex, self.settings.show_decoded)
        {
            match action {
                ExportAction::CopyToClipboard(content) => {
                    ui.output_mut(|o| o.copied_text = content);
                    self.set_status("Log copied to clipboard".to_string());
                }
                ExportAction::SavedToFile(path) => {
                    self.set_status(format!("Log saved to {}", path.display()));
                }
                ExportAction::Cancelled => {
                    // User cancelled, do nothing
                }
                ExportAction::Error(e) => {
                    self.report_err("Export", e);
                }
            }
        }

        // Sync diagnostic level to settings and update tracing filter if changed
        let current_level = self.traffic_monitor.diagnostic_level();
        if self.prev_diagnostic_level != current_level {
            // Update settings
            self.settings.diagnostic_level = current_level;
            if let Err(e) = self.settings.save() {
                self.handle_save_error(e);
            }

            // Update the tracing filter dynamically (atomic store, no parsing)
            self.diagnostic_level_state.set_level(current_level);

            // Track the change
            self.prev_diagnostic_level = current_level;
        }
    }
}
