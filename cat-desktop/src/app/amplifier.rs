//! Amplifier connection and management

use std::time::Duration;

use cat_mux::{AmplifierChannel, AmplifierChannelMeta, AsyncAmpConnection, MuxActorCommand};
use cat_protocol::Protocol;
use cat_sim::{run_virtual_amp_task, VirtualAmpCommand, VirtualAmplifier};
use egui::{Color32, RichText, Ui};
use tokio::sync::{broadcast, mpsc as tokio_mpsc, oneshot};
use tokio_serial::SerialPortBuilderExt;

use super::{AmplifierConnectionType, CatapultApp};

impl CatapultApp {
    /// Draw the amplifier configuration panel
    pub(super) fn draw_amplifier_panel(&mut self, ui: &mut Ui) {
        // Capture previous state for change detection
        let prev_connection_type = self.amp_connection_type;
        let prev_protocol = self.amp_protocol;
        let prev_port = self.amp_port.clone();
        let prev_baud = self.amp_baud;
        let prev_civ = self.amp_civ_address;

        egui::Grid::new("amp_config")
            .num_columns(2)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                // Connection type selector
                ui.label("Connection:");
                egui::ComboBox::from_id_salt("amp_connection_type")
                    .selected_text(match self.amp_connection_type {
                        AmplifierConnectionType::ComPort => "COM Port",
                        AmplifierConnectionType::Simulated => "Simulated",
                    })
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_value(
                                &mut self.amp_connection_type,
                                AmplifierConnectionType::ComPort,
                                "COM Port",
                            )
                            .changed()
                        {
                            // Disconnect when switching to COM port mode
                            if self.amp_data_tx.is_some() {
                                self.disconnect_amplifier();
                            }
                        }
                        ui.selectable_value(
                            &mut self.amp_connection_type,
                            AmplifierConnectionType::Simulated,
                            "Simulated",
                        );
                    });
                ui.end_row();

                ui.label("Protocol:");
                egui::ComboBox::from_id_salt("amp_protocol")
                    .selected_text(self.amp_protocol.name())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.amp_protocol,
                            Protocol::Kenwood,
                            Protocol::Kenwood.name(),
                        );
                        ui.selectable_value(
                            &mut self.amp_protocol,
                            Protocol::IcomCIV,
                            Protocol::IcomCIV.name(),
                        );
                        ui.selectable_value(
                            &mut self.amp_protocol,
                            Protocol::Yaesu,
                            Protocol::Yaesu.name(),
                        );
                        ui.selectable_value(
                            &mut self.amp_protocol,
                            Protocol::YaesuAscii,
                            Protocol::YaesuAscii.name(),
                        );
                        ui.selectable_value(
                            &mut self.amp_protocol,
                            Protocol::Elecraft,
                            Protocol::Elecraft.name(),
                        );
                    });
                ui.end_row();

                // Only show port/baud for COM port mode
                if self.amp_connection_type == AmplifierConnectionType::ComPort {
                    ui.label("Port:");
                    // Get available ports (excludes ports used by radios)
                    // Collect into owned data to avoid borrow conflicts
                    let available_amp_ports: Vec<(String, String)> = self
                        .available_amp_ports()
                        .into_iter()
                        .map(|p| (p.port.clone(), Self::format_port_label(p)))
                        .collect();

                    // Find the selected port's hint for display
                    let selected_label = if self.amp_port.is_empty() {
                        "Select port...".to_string()
                    } else {
                        available_amp_ports
                            .iter()
                            .find(|(port, _)| *port == self.amp_port)
                            .map(|(_, label)| label.clone())
                            .unwrap_or_else(|| self.amp_port.clone())
                    };
                    egui::ComboBox::from_id_salt("amp_port")
                        .selected_text(selected_label)
                        .show_ui(ui, |ui| {
                            for (port, label) in &available_amp_ports {
                                ui.selectable_value(&mut self.amp_port, port.clone(), label);
                            }
                        });
                    ui.end_row();

                    ui.label("Baud Rate:");
                    egui::ComboBox::from_id_salt("amp_baud")
                        .selected_text(format!("{}", self.amp_baud))
                        .show_ui(ui, |ui| {
                            // Common amplifier baud rates
                            for &baud in &[4800u32, 9600, 19200, 38400, 57600, 115200, 230400] {
                                ui.selectable_value(&mut self.amp_baud, baud, format!("{}", baud));
                            }
                        });
                    ui.end_row();

                    // Show CI-V address for Icom protocol
                    if self.amp_protocol == Protocol::IcomCIV {
                        ui.label("CI-V Address:");
                        let mut addr_str = format!("{:02X}", self.amp_civ_address);
                        if ui.text_edit_singleline(&mut addr_str).changed() {
                            if let Ok(addr) =
                                u8::from_str_radix(addr_str.trim_start_matches("0x"), 16)
                            {
                                self.amp_civ_address = addr;
                            }
                        }
                        ui.end_row();
                    }
                }
            });

        // Status and controls based on connection type
        match self.amp_connection_type {
            AmplifierConnectionType::ComPort => {
                ui.horizontal(|ui| {
                    let is_connected = self.amp_data_tx.is_some();
                    let can_connect = !self.amp_port.is_empty() && !is_connected;

                    if ui
                        .add_enabled(can_connect, egui::Button::new("Connect"))
                        .clicked()
                    {
                        self.connect_amplifier();
                    }

                    if ui
                        .add_enabled(is_connected, egui::Button::new("Disconnect"))
                        .clicked()
                    {
                        self.disconnect_amplifier();
                    }

                    if is_connected {
                        ui.label(RichText::new("Connected").color(Color32::GREEN));
                    } else if !self.amp_port.is_empty() {
                        ui.label(RichText::new("Disconnected").color(Color32::GRAY));
                    }
                });
            }
            AmplifierConnectionType::Simulated => {
                // Connection status and controls
                ui.horizontal(|ui| {
                    let is_connected = self.amp_data_tx.is_some();

                    if ui
                        .add_enabled(!is_connected, egui::Button::new("Connect"))
                        .clicked()
                    {
                        self.connect_amplifier();
                    }

                    if ui
                        .add_enabled(is_connected, egui::Button::new("Disconnect"))
                        .clicked()
                    {
                        self.disconnect_amplifier();
                    }

                    if is_connected {
                        ui.label(RichText::new("Connected").color(Color32::GREEN));
                    } else {
                        ui.label(RichText::new("Disconnected").color(Color32::GRAY));
                    }
                });

                // Only show state when connected
                if self.amp_data_tx.is_some() {
                    ui.add_space(8.0);
                    ui.separator();

                    // Emulated state display
                    ui.label(RichText::new("Amplifier State:").strong());

                    egui::Grid::new("virtual_amp_state")
                        .num_columns(2)
                        .spacing([10.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("Freq:");
                            let freq_str = match self.virtual_amp_state.as_ref() {
                                Some(state) => {
                                    let mhz = state.frequency_hz as f64 / 1_000_000.0;
                                    format!("{:.3} MHz", mhz)
                                }
                                None => "---".to_string(),
                            };
                            ui.label(RichText::new(freq_str).monospace());
                            ui.end_row();

                            ui.label("Mode:");
                            let mode_str = match self.virtual_amp_state.as_ref() {
                                Some(state) => super::mode_name(state.mode).to_string(),
                                None => "---".to_string(),
                            };
                            ui.label(RichText::new(mode_str).monospace());
                            ui.end_row();

                            ui.label("PTT:");
                            let (ptt_str, ptt_color) = match self.virtual_amp_state.as_ref() {
                                Some(state) if state.ptt => ("TX", Color32::RED),
                                Some(_) => ("RX", Color32::GREEN),
                                None => ("---", Color32::GRAY),
                            };
                            ui.label(RichText::new(ptt_str).monospace().color(ptt_color));
                            ui.end_row();
                        });
                }
            }
        }

        // Save if any amplifier settings changed
        if self.amp_connection_type != prev_connection_type
            || self.amp_protocol != prev_protocol
            || self.amp_port != prev_port
            || self.amp_baud != prev_baud
            || self.amp_civ_address != prev_civ
        {
            self.save_amplifier_settings();
        }
    }

    /// Connect to the amplifier (handles both COM and virtual based on connection type)
    pub(super) fn connect_amplifier(&mut self) {
        let civ_address = if self.amp_protocol == Protocol::IcomCIV {
            Some(self.amp_civ_address)
        } else {
            None
        };

        // Determine port name, baud rate, and metadata based on connection type
        let (port_name, baud_rate, amp_meta) = match self.amp_connection_type {
            AmplifierConnectionType::ComPort => {
                if self.amp_port.is_empty() {
                    self.set_status("No amplifier port selected".into());
                    return;
                }
                (
                    self.amp_port.clone(),
                    self.amp_baud,
                    AmplifierChannelMeta::new_real(
                        self.amp_port.clone(),
                        self.amp_protocol,
                        self.amp_baud,
                        civ_address,
                    ),
                )
            }
            AmplifierConnectionType::Simulated => (
                "[VIRTUAL]".to_string(),
                0,
                AmplifierChannelMeta::new_virtual(self.amp_protocol, civ_address),
            ),
        };

        // Send config to mux actor
        self.send_mux_command(
            MuxActorCommand::SetAmplifierConfig {
                port: port_name.clone(),
                protocol: self.amp_protocol,
                baud_rate,
                civ_address,
            },
            "SetAmplifierConfig",
        );

        // Create channels
        let (amp_data_tx, amp_data_rx) = tokio_mpsc::channel::<Vec<u8>>(64);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (_response_tx, response_rx) = tokio_mpsc::channel::<Vec<u8>>(64);

        // Create AmplifierChannel and tell mux actor
        let amp_channel = AmplifierChannel::new(amp_meta, amp_data_tx.clone(), response_rx);
        self.send_mux_command(
            MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            },
            "ConnectAmplifier",
        );

        // Store senders
        self.amp_data_tx = Some(amp_data_tx);
        self.amp_shutdown_tx = Some(shutdown_tx);

        // Spawn the async amp connection based on connection type
        let mux_tx = self.mux_cmd_tx.clone();
        let event_tx = self.mux_event_tx.clone();

        match self.amp_connection_type {
            AmplifierConnectionType::ComPort => {
                // Open the serial port
                let stream = match tokio_serial::new(&self.amp_port, self.amp_baud)
                    .timeout(Duration::from_millis(100))
                    .open_native_async()
                {
                    Ok(s) => s,
                    Err(e) => {
                        self.set_status(format!("Failed to open {}: {}", self.amp_port, e));
                        // Clean up state on failure
                        self.amp_data_tx = None;
                        self.amp_shutdown_tx = None;
                        self.send_mux_command(
                            MuxActorCommand::DisconnectAmplifier,
                            "DisconnectAmplifier (cleanup)",
                        );
                        return;
                    }
                };

                self.rt_handle.spawn(async move {
                    let conn = AsyncAmpConnection::new(stream, mux_tx, event_tx);
                    conn.run(shutdown_rx, amp_data_rx).await;
                });

                self.set_status(format!(
                    "Connected to amplifier on {} @ {} baud",
                    self.amp_port, self.amp_baud
                ));
            }
            AmplifierConnectionType::Simulated => {
                // Create duplex stream pair - one end for mux, one for virtual amp actor
                let (mux_stream, amp_stream) = tokio::io::duplex(4096);

                // Create virtual amplifier
                let virtual_amp = VirtualAmplifier::new("virtual-amp", self.amp_protocol, civ_address);

                // Create channels for virtual amp actor
                let (vamp_cmd_tx, vamp_cmd_rx) = tokio_mpsc::channel::<VirtualAmpCommand>(32);
                let (vamp_state_tx, vamp_state_rx) = broadcast::channel::<cat_sim::VirtualAmpStateEvent>(32);

                // Store senders/receivers for virtual amp
                self.virtual_amp_cmd_tx = Some(vamp_cmd_tx);
                self.virtual_amp_state_rx = Some(vamp_state_rx);

                // Spawn the virtual amp actor task
                self.rt_handle.spawn(async move {
                    if let Err(e) = run_virtual_amp_task(amp_stream, virtual_amp, vamp_cmd_rx, vamp_state_tx).await {
                        tracing::error!("Virtual amplifier task error: {}", e);
                    }
                });

                // Spawn the AsyncAmpConnection with the mux side of the duplex
                self.rt_handle.spawn(async move {
                    let conn = AsyncAmpConnection::new(mux_stream, mux_tx, event_tx);
                    conn.run(shutdown_rx, amp_data_rx).await;
                });

                self.set_status(format!(
                    "Connected to virtual amplifier (protocol: {})",
                    self.amp_protocol.name()
                ));
            }
        }
    }

    /// Disconnect from the amplifier
    pub(super) fn disconnect_amplifier(&mut self) {
        // Tell mux actor to stop sending to amp
        self.send_mux_command(MuxActorCommand::DisconnectAmplifier, "DisconnectAmplifier");

        // Send shutdown to virtual amp task if connected
        if let Some(tx) = self.virtual_amp_cmd_tx.take() {
            let _ = tx.try_send(VirtualAmpCommand::Shutdown);
        }

        // Send shutdown to amp connection task
        if let Some(tx) = self.amp_shutdown_tx.take() {
            let _ = tx.send(());
        }

        self.amp_data_tx = None;
        self.virtual_amp_state_rx = None;
        self.virtual_amp_state = None;
        self.set_status("Amplifier disconnected".into());
    }
}
