//! Main application state and UI

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use cat_detect::{DetectedRadio, PortScanner, SerialPortInfo};
use cat_mux::{Multiplexer, MultiplexerEvent, RadioHandle, SwitchingMode};
use cat_protocol::{Protocol, RadioCommand};
use cat_sim::SimulationEvent;
use eframe::CreationContext;
use egui::{Color32, RichText, Ui};

use crate::firmware_panel::FirmwarePanel;
use crate::radio_panel::RadioPanel;
use crate::serial_io::AmplifierConnection;
use crate::settings::Settings;
use crate::simulation_panel::SimulationPanel;
use crate::traffic_monitor::TrafficMonitor;

/// Messages from background tasks
pub enum BackgroundMessage {
    /// Scan completed
    ScanComplete(Vec<DetectedRadio>),
    /// Error occurred
    Error(String),
    /// Traffic received
    TrafficIn { radio: RadioHandle, data: Vec<u8> },
    /// Traffic sent
    TrafficOut { data: Vec<u8> },
}

/// Main application state
pub struct CatapultApp {
    /// Settings
    settings: Settings,
    /// Multiplexer engine
    multiplexer: Multiplexer,
    /// Port scanner
    scanner: PortScanner,
    /// Available serial ports
    available_ports: Vec<SerialPortInfo>,
    /// Detected radios from last scan
    detected_radios: Vec<DetectedRadio>,
    /// Radio panels for UI
    radio_panels: Vec<RadioPanel>,
    /// Traffic monitor
    traffic_monitor: TrafficMonitor,
    /// Is scanning in progress
    scanning: bool,
    /// Last scan time
    last_scan: Option<Instant>,
    /// Status message
    status_message: Option<(String, Instant)>,
    /// Show settings panel
    show_settings: bool,
    /// Show firmware panel
    show_firmware: bool,
    /// Firmware panel
    firmware_panel: FirmwarePanel,
    /// Show simulation panel
    show_simulation: bool,
    /// Simulation panel for virtual radios
    simulation_panel: SimulationPanel,
    /// Background message receiver
    bg_rx: Receiver<BackgroundMessage>,
    /// Background message sender (for cloning to tasks)
    bg_tx: Sender<BackgroundMessage>,
    /// Selected amplifier port
    amp_port: String,
    /// Selected amplifier protocol
    amp_protocol: Protocol,
    /// Selected amplifier baud rate
    amp_baud: u32,
    /// CI-V address for Icom amplifiers (0x00-0xFF)
    amp_civ_address: u8,
    /// Amplifier connection (when connected)
    amp_connection: Option<AmplifierConnection>,
    /// Maps simulation radio IDs to multiplexer handles
    sim_radio_handles: HashMap<String, RadioHandle>,
}

impl CatapultApp {
    /// Create a new application
    pub fn new(_cc: &CreationContext<'_>) -> Self {
        let (bg_tx, bg_rx) = mpsc::channel();
        let settings = Settings::load();

        let mut app = Self {
            traffic_monitor: TrafficMonitor::new(
                settings.traffic_history_size,
                settings.show_hex,
                settings.show_decoded,
            ),
            settings,
            multiplexer: Multiplexer::new(),
            scanner: PortScanner::new(),
            available_ports: Vec::new(),
            detected_radios: Vec::new(),
            radio_panels: Vec::new(),
            scanning: false,
            last_scan: None,
            status_message: None,
            show_settings: false,
            show_firmware: false,
            firmware_panel: FirmwarePanel::new(),
            show_simulation: false,
            simulation_panel: SimulationPanel::new(),
            bg_rx,
            bg_tx,
            amp_port: String::new(),
            amp_protocol: Protocol::Kenwood,
            amp_baud: 9600,
            amp_civ_address: 0x00,
            amp_connection: None,
            sim_radio_handles: HashMap::new(),
        };

        // Initial port enumeration
        app.refresh_ports();

        // Auto-scan on startup if enabled
        if app.settings.auto_scan {
            app.start_scan();
        }

        app
    }

    /// Refresh available ports
    fn refresh_ports(&mut self) {
        match self.scanner.enumerate_ports() {
            Ok(ports) => {
                self.available_ports = ports;
            }
            Err(e) => {
                self.set_status(format!("Failed to enumerate ports: {}", e));
            }
        }
    }

    /// Set a status message
    fn set_status(&mut self, msg: String) {
        self.status_message = Some((msg, Instant::now()));
    }

    /// Process background messages
    fn process_messages(&mut self) {
        while let Ok(msg) = self.bg_rx.try_recv() {
            match msg {
                BackgroundMessage::ScanComplete(radios) => {
                    self.scanning = false;
                    self.last_scan = Some(Instant::now());

                    // Add detected radios to multiplexer
                    for radio in &radios {
                        let handle = self.multiplexer.add_radio(
                            radio.model_name(),
                            radio.port.clone(),
                            radio.protocol,
                        );

                        // Update radio state from detection
                        if let Some(state) = self.multiplexer.get_radio_mut(handle) {
                            state.update_from_detection(radio);
                        }

                        self.radio_panels.push(RadioPanel::new(handle, radio));
                    }

                    self.detected_radios = radios;
                    self.set_status("Scan complete".into());
                }
                BackgroundMessage::Error(e) => {
                    self.scanning = false;
                    self.set_status(format!("Error: {}", e));
                }
                BackgroundMessage::TrafficIn { radio, data } => {
                    self.traffic_monitor.add_incoming(radio, &data);
                }
                BackgroundMessage::TrafficOut { data } => {
                    self.traffic_monitor.add_outgoing(&data);
                }
            }
        }
    }

    /// Process multiplexer events
    fn process_mux_events(&mut self) {
        for event in self.multiplexer.drain_events() {
            match event {
                MultiplexerEvent::ActiveRadioChanged { from: _, to } => {
                    let name = self
                        .multiplexer
                        .get_radio(to)
                        .map(|r| r.name.clone())
                        .unwrap_or_default();
                    self.set_status(format!("Switched to {}", name));
                }
                MultiplexerEvent::AmplifierCommand(data) => {
                    // Send to amplifier if connected, otherwise log as simulated
                    if let Some(ref mut conn) = self.amp_connection {
                        self.traffic_monitor.add_outgoing(&data);
                        if let Err(e) = conn.write(&data) {
                            self.set_status(format!("Amplifier write error: {}", e));
                        }
                    } else {
                        self.traffic_monitor.add_simulated_outgoing(&data);
                    }
                }
                MultiplexerEvent::Error(e) => {
                    self.set_status(format!("Error: {}", e));
                }
                _ => {}
            }
        }
    }

    /// Draw the toolbar
    fn draw_toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.scanning, egui::Button::new("Scan Ports"))
                .clicked()
            {
                self.start_scan();
            }

            if self.scanning {
                ui.spinner();
                ui.label("Scanning...");
            }

            ui.separator();

            if ui.button("Settings").clicked() {
                self.show_settings = !self.show_settings;
            }

            if ui.button("Firmware").clicked() {
                self.show_firmware = !self.show_firmware;
            }

            // Simulation mode button (only shown when enabled in settings)
            if self.settings.debug_mode {
                let sim_button = egui::Button::new("Simulate").fill(if self.show_simulation {
                    Color32::from_rgb(80, 80, 40)
                } else {
                    Color32::TRANSPARENT
                });
                if ui.add(sim_button).clicked() {
                    self.show_simulation = !self.show_simulation;
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Active indicator
                if self.multiplexer.active_radio().is_some() {
                    ui.label(RichText::new("●").color(Color32::GREEN).size(16.0));
                    ui.label("Active");
                } else {
                    ui.label(RichText::new("○").color(Color32::GRAY).size(16.0));
                    ui.label("No radio");
                }
            });
        });
    }

    /// Draw the radio list panel
    fn draw_radio_panel(&mut self, ui: &mut Ui) {
        ui.heading("Radios");

        let has_real_radios = !self.radio_panels.is_empty();
        let has_sim_radios =
            self.settings.debug_mode && self.simulation_panel.context().radio_count() > 0;

        if !has_real_radios && !has_sim_radios {
            ui.label("No radios detected. Click 'Scan Ports' to search.");
            if self.settings.debug_mode {
                ui.label(
                    RichText::new("Or use the Simulate panel to add virtual radios.")
                        .color(Color32::GRAY)
                        .small(),
                );
            }
            return;
        }

        // Draw real radios
        if has_real_radios {
            let active = self.multiplexer.active_radio();

            // Collect radio info to avoid borrow conflicts
            let radio_info: Vec<_> = self
                .radio_panels
                .iter()
                .map(|panel| {
                    let state = self.multiplexer.get_radio(panel.handle);
                    let freq_display = state.map(|s| s.frequency_display()).unwrap_or_default();
                    let mode_display = state.map(|s| s.mode_display()).unwrap_or_default();
                    let ptt = state.map(|s| s.ptt).unwrap_or(false);
                    (
                        panel.handle,
                        panel.name.clone(),
                        panel.port.clone(),
                        freq_display,
                        mode_display,
                        ptt,
                    )
                })
                .collect();

            let mut selected_handle = None;

            for (handle, name, port, freq_display, mode_display, ptt) in &radio_info {
                let is_active = active == Some(*handle);

                // Determine background color based on state
                let bg_color = if *ptt {
                    Color32::from_rgb(80, 30, 30) // Red tint when transmitting
                } else if is_active {
                    Color32::from_rgb(40, 60, 40)
                } else {
                    Color32::from_rgb(30, 30, 30)
                };

                egui::Frame::none()
                    .fill(bg_color)
                    .rounding(4.0)
                    .inner_margin(8.0)
                    .outer_margin(4.0)
                    .show(ui, |ui| {
                        // Top row: TX indicator (if transmitting) and Select button
                        ui.horizontal(|ui| {
                            if *ptt {
                                ui.label(
                                    RichText::new("● TX")
                                        .color(Color32::from_rgb(255, 80, 80))
                                        .strong()
                                        .size(14.0),
                                );
                            }

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if !is_active && ui.button("Select").clicked() {
                                        selected_handle = Some(*handle);
                                    }
                                },
                            );
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

                        // Radio name and port - small, secondary
                        ui.horizontal(|ui| {
                            if is_active {
                                ui.label(RichText::new("●").color(Color32::GREEN).size(10.0));
                            }
                            ui.label(
                                RichText::new(format!("{} · {}", name, port))
                                    .color(Color32::GRAY)
                                    .size(11.0),
                            );
                        });
                    });
            }

            // Handle selection after the loop to avoid borrow conflicts
            if let Some(handle) = selected_handle {
                let _ = self.multiplexer.select_radio(handle);
            }
        }

        // Draw simulated radios (when debug mode is enabled)
        if has_sim_radios {
            if has_real_radios {
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
            }

            ui.label(
                RichText::new("Simulated Radios")
                    .color(Color32::from_rgb(255, 165, 0))
                    .strong(),
            );

            // Get the multiplexer's active radio handle
            let active_mux = self.multiplexer.active_radio();

            // Collect sim radio info with their multiplexer handles
            let sim_radios: Vec<_> = self
                .simulation_panel
                .context()
                .radios()
                .map(|(id, radio)| {
                    let freq = radio.frequency_hz() as f64 / 1_000_000.0;
                    let mux_handle = self.sim_radio_handles.get(id).copied();
                    (
                        id.clone(),
                        radio.id().to_string(),
                        radio.protocol(),
                        format!("{:.3} MHz", freq),
                        format!("{:?}", radio.mode()),
                        radio.ptt(),
                        mux_handle,
                    )
                })
                .collect();

            for (_id, name, protocol, freq_display, mode_display, ptt, mux_handle) in &sim_radios {
                // Check if this sim radio is the active one in the multiplexer
                let is_active = mux_handle.is_some() && active_mux == *mux_handle;

                // Determine background color based on state
                let bg_color = if *ptt {
                    Color32::from_rgb(80, 40, 20) // Red-orange tint when transmitting
                } else if is_active {
                    Color32::from_rgb(60, 50, 30)
                } else {
                    Color32::from_rgb(40, 35, 25)
                };

                egui::Frame::none()
                    .fill(bg_color)
                    .rounding(4.0)
                    .inner_margin(8.0)
                    .outer_margin(4.0)
                    .show(ui, |ui| {
                        // Top row: SIM badge, TX indicator (if transmitting)
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("[SIM]")
                                    .color(Color32::from_rgb(255, 165, 0))
                                    .strong()
                                    .size(10.0),
                            );
                            if *ptt {
                                ui.label(
                                    RichText::new("● TX")
                                        .color(Color32::from_rgb(255, 80, 80))
                                        .strong()
                                        .size(14.0),
                                );
                            }
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

                        // Radio name and protocol - small, secondary
                        ui.horizontal(|ui| {
                            if is_active {
                                ui.label(RichText::new("●").color(Color32::GREEN).size(10.0));
                            }
                            ui.label(
                                RichText::new(format!("{} · {}", name, protocol.name()))
                                    .color(Color32::GRAY)
                                    .size(11.0),
                            );
                        });
                    });
            }
        }
    }

    /// Draw the amplifier configuration panel
    fn draw_amplifier_panel(&mut self, ui: &mut Ui) {
        ui.heading("Amplifier");

        egui::Grid::new("amp_config")
            .num_columns(2)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                ui.label("Protocol:");
                egui::ComboBox::from_id_salt("amp_protocol")
                    .selected_text(self.amp_protocol.name())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.amp_protocol, Protocol::Kenwood, "Kenwood");
                        ui.selectable_value(&mut self.amp_protocol, Protocol::IcomCIV, "Icom CI-V");
                        ui.selectable_value(&mut self.amp_protocol, Protocol::Yaesu, "Yaesu");
                        ui.selectable_value(&mut self.amp_protocol, Protocol::Elecraft, "Elecraft");
                    });
                ui.end_row();

                ui.label("Port:");
                egui::ComboBox::from_id_salt("amp_port")
                    .selected_text(if self.amp_port.is_empty() {
                        "Select port..."
                    } else {
                        &self.amp_port
                    })
                    .show_ui(ui, |ui| {
                        for port in &self.available_ports {
                            ui.selectable_value(&mut self.amp_port, port.port.clone(), &port.port);
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
                        if let Ok(addr) = u8::from_str_radix(addr_str.trim_start_matches("0x"), 16)
                        {
                            self.amp_civ_address = addr;
                        }
                    }
                    ui.end_row();
                }
            });

        ui.horizontal(|ui| {
            let is_connected = self.amp_connection.is_some();
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
                ui.label(RichText::new("● Connected").color(Color32::GREEN));
            } else if !self.amp_port.is_empty() {
                ui.label(RichText::new("● Disconnected").color(Color32::GRAY));
            }
        });
    }

    /// Draw the switching mode panel
    fn draw_switching_panel(&mut self, ui: &mut Ui) {
        ui.heading("Switching");

        let mut mode = self.multiplexer.switching_mode();

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
                                self.multiplexer.set_switching_mode(mode);
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

        if self.multiplexer.is_locked() {
            ui.horizontal(|ui| {
                ui.label("Lockout:");
                ui.label(format!("{}ms", self.multiplexer.lockout_remaining_ms()));
            });
        }
    }

    /// Draw the traffic monitor panel
    fn draw_traffic_panel(&mut self, ui: &mut Ui) {
        ui.heading("Traffic Monitor");

        self.traffic_monitor.draw(ui);
    }

    /// Start a port scan
    fn start_scan(&mut self) {
        self.scanning = true;
        self.set_status("Scanning ports...".into());

        // Clear existing radios
        self.radio_panels.clear();
        let handles: Vec<_> = self.multiplexer.radios().map(|r| r.handle).collect();
        for handle in handles {
            self.multiplexer.remove_radio(handle);
        }

        // Spawn background thread for async scanning
        let tx = self.bg_tx.clone();
        std::thread::spawn(move || {
            // Create a tokio runtime for the async scan
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(BackgroundMessage::Error(format!(
                        "Failed to create runtime: {}",
                        e
                    )));
                    return;
                }
            };

            // Run the scan
            let result = rt.block_on(async {
                let mut scanner = PortScanner::new();
                scanner.scan().await
            });

            // Send results back
            let _ = tx.send(BackgroundMessage::ScanComplete(result));
        });
    }

    /// Connect to the amplifier
    fn connect_amplifier(&mut self) {
        if self.amp_port.is_empty() {
            self.set_status("No amplifier port selected".into());
            return;
        }

        // Update multiplexer config with amplifier settings
        let amp_config = cat_mux::state::AmplifierConfig {
            port: self.amp_port.clone(),
            protocol: self.amp_protocol,
            baud_rate: self.amp_baud,
            civ_address: if self.amp_protocol == Protocol::IcomCIV {
                Some(self.amp_civ_address)
            } else {
                None
            },
        };
        self.multiplexer.set_amplifier_config(amp_config);

        match AmplifierConnection::new(&self.amp_port, self.amp_baud, self.bg_tx.clone()) {
            Ok(conn) => {
                self.amp_connection = Some(conn);
                self.set_status(format!(
                    "Connected to amplifier on {} @ {} baud",
                    self.amp_port, self.amp_baud
                ));
            }
            Err(e) => {
                self.set_status(format!("Failed to connect: {}", e));
            }
        }
    }

    /// Disconnect from the amplifier
    fn disconnect_amplifier(&mut self) {
        self.amp_connection = None;
        self.set_status("Amplifier disconnected".into());
    }

    /// Process simulation events and update traffic monitor
    fn process_simulation_events(&mut self) {
        for event in self.simulation_panel.drain_events() {
            match event {
                SimulationEvent::RadioOutput { radio_id, data } => {
                    // Add to traffic monitor as simulated incoming
                    self.traffic_monitor.add_simulated_incoming(radio_id, &data);
                }
                SimulationEvent::RadioAdded { radio_id } => {
                    // Register the simulated radio with the multiplexer
                    if let Some(radio) = self.simulation_panel.context().get_radio(&radio_id) {
                        let handle = self.multiplexer.add_radio(
                            radio.id().to_string(),
                            "SIM".to_string(),
                            radio.protocol(),
                        );
                        self.sim_radio_handles.insert(radio_id.clone(), handle);
                    }
                    self.set_status(format!("Virtual radio added: {}", radio_id));
                }
                SimulationEvent::RadioRemoved { radio_id } => {
                    // Remove the simulated radio from the multiplexer
                    if let Some(handle) = self.sim_radio_handles.remove(&radio_id) {
                        self.multiplexer.remove_radio(handle);
                    }
                    self.set_status(format!("Virtual radio removed: {}", radio_id));
                }
                SimulationEvent::RadioStateChanged {
                    radio_id,
                    frequency_hz,
                    mode,
                    ptt,
                } => {
                    // Feed state changes to the multiplexer to trigger auto-switching
                    if let Some(&handle) = self.sim_radio_handles.get(&radio_id) {
                        if let Some(hz) = frequency_hz {
                            self.multiplexer
                                .process_radio_command(handle, RadioCommand::SetFrequency { hz });
                        }
                        if let Some(m) = mode {
                            self.multiplexer
                                .process_radio_command(handle, RadioCommand::SetMode { mode: m });
                        }
                        if let Some(active) = ptt {
                            self.multiplexer
                                .process_radio_command(handle, RadioCommand::SetPtt { active });
                        }
                    }
                }
            }
        }
    }
}

impl eframe::App for CatapultApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process background messages
        self.process_messages();
        self.process_simulation_events();
        self.process_mux_events();

        // Clear old status messages
        if let Some((_, when)) = &self.status_message {
            if when.elapsed().as_secs() > 5 {
                self.status_message = None;
            }
        }

        // Top panel - toolbar
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            self.draw_toolbar(ui);
        });

        // Bottom panel - status bar
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Debug mode indicator
                if self.settings.debug_mode {
                    ui.label(RichText::new("[DEBUG]").color(Color32::YELLOW).strong());
                    ui.separator();
                }

                if let Some((msg, _)) = &self.status_message {
                    ui.label(msg);
                } else {
                    ui.label("Ready");
                }

                // Show amplifier status on the right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.amp_connection.is_some() {
                        ui.label(RichText::new("Amp: Connected").color(Color32::GREEN));
                    } else if self.settings.debug_mode && self.show_simulation {
                        ui.label(RichText::new("Amp: Simulation").color(Color32::LIGHT_BLUE));
                    }
                });
            });
        });

        // Settings panel (side panel)
        if self.show_settings {
            egui::SidePanel::right("settings")
                .default_width(300.0)
                .show(ctx, |ui| {
                    ui.heading("Settings");
                    ui.separator();

                    self.settings.draw(ui);

                    ui.separator();
                    if ui.button("Close").clicked() {
                        self.show_settings = false;
                    }
                });
        }

        // Firmware panel (side panel)
        if self.show_firmware {
            egui::SidePanel::right("firmware")
                .default_width(320.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.firmware_panel.draw(ui);

                        ui.add_space(16.0);
                        ui.separator();
                        if ui.button("Close").clicked() {
                            self.show_firmware = false;
                        }
                    });
                });
        }

        // Simulation panel (side panel, only when debug mode enabled)
        if self.show_simulation && self.settings.debug_mode {
            egui::SidePanel::right("simulation")
                .default_width(380.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.simulation_panel.draw(ui);

                        ui.add_space(16.0);
                        ui.separator();
                        if ui.button("Close").clicked() {
                            self.show_simulation = false;
                        }
                    });
                });
        }

        // Left panel - radio list
        egui::SidePanel::left("radios")
            .default_width(280.0)
            .min_width(200.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.draw_radio_panel(ui);

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(8.0);

                    self.draw_amplifier_panel(ui);

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(8.0);

                    self.draw_switching_panel(ui);
                });
            });

        // Central panel - traffic monitor
        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_traffic_panel(ui);
        });

        // Request repaint only when animations are active
        if self.scanning || self.show_firmware || self.show_simulation {
            ctx.request_repaint();
        }
    }
}
